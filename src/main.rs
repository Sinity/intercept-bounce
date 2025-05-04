// Orchestrates command-line parsing, thread setup, the main event loop,
// signal handling, and final shutdown/stats reporting.

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::io::{self, ErrorKind};
use std::os::fd::RawFd; // Import RawFd
use std::os::unix::io::AsRawFd;
use std::process::exit;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use event::{event_microseconds, list_input_devices, read_event_raw, write_event_raw};
use filter::stats::StatsCollector;
use filter::BounceFilter;
use intercept_bounce::event;
use intercept_bounce::filter;
use intercept_bounce::logger;
use intercept_bounce::{cli, config::Config, util};
use logger::{EventInfo, LogMessage, Logger};
use tracing::{debug, error, info, instrument, trace, warn}; // Removed Level, Span

// --- OTLP Imports ---
use opentelemetry::global as otel_global;
use opentelemetry::metrics::{Meter, MeterProvider as _}; // Import Meter trait and MeterProvider trait
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{metrics::SdkMeterProvider, runtime, trace as sdktrace, Resource};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter}; // Import the crate

// Define capacity constant
const LOGGER_QUEUE_CAPACITY: usize = 1024; // Or make configurable

/// State specific to the main processing thread for managing communication
/// with the logger thread and handling log drop warnings.
struct MainState {
    log_sender: Sender<LogMessage>, // Use Sender directly
    warned_about_dropping: bool,
    currently_dropping: bool,
    total_dropped_log_messages: u64,
}

// --- Structs to group arguments for run_main_loop ---

/// Holds context information passed to the main event loop.
struct MainLoopContext<'a> {
    main_running: &'a Arc<AtomicBool>,
    stdin_fd: RawFd,
    stdout_fd: RawFd,
    bounce_filter: &'a Arc<Mutex<BounceFilter>>,
    cfg: &'a Arc<Config>,
    check_interval: Duration,
}

/// Holds the optional OpenTelemetry counters.
struct OtelCounters {
    events_processed: Option<opentelemetry::metrics::Counter<u64>>,
    events_passed: Option<opentelemetry::metrics::Counter<u64>>,
    events_dropped: Option<opentelemetry::metrics::Counter<u64>>,
    log_messages_dropped: Option<opentelemetry::metrics::Counter<u64>>,
}


/// Attempts to set the process priority to the highest level (-20 niceness).
/// Prints a warning if it fails (e.g., due to insufficient permissions).
fn set_high_priority() {
    #[cfg(target_os = "linux")]
    {
        debug!("Attempting to set high process priority (niceness -20)...");
        let res = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -20) };
        if res != 0 {
            warn!(
                "Unable to set process niceness to -20 (requires root or CAP_SYS_NICE). Error: {}",
                io::Error::last_os_error()
            );
        } else {
            info!("Process priority set to -20 (highest).");
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        info!("set_high_priority is only implemented for Linux.");
    }
}

// --- OTLP Initialization ---
// Update function signature and return type to use the aliased SdkMeterProvider
fn init_otel(cfg: &Config) -> Option<(SdkMeterProvider, sdktrace::Tracer, Meter)> {
    let otel_endpoint = cfg.otel_endpoint.as_ref()?;
    info!(endpoint = %otel_endpoint, "Initializing OpenTelemetry exporter...");

    // --- Trace Pipeline ---
    let trace_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(otel_endpoint);
    let trace_config = sdktrace::config().with_resource(Resource::new(vec![
        opentelemetry::KeyValue::new("service.name", "intercept-bounce"),
        opentelemetry::KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
    ]));
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(trace_exporter)
        .with_trace_config(trace_config)
        .install_batch(runtime::TokioCurrentThread) // Use the chosen runtime
        .map_err(|e| error!(error = %e, "Failed to initialize OTLP trace pipeline"))
        .ok()?;

    // --- Metrics Pipeline ---
    let metrics_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(otel_endpoint);
    let meter_provider = opentelemetry_otlp::new_pipeline()
        .metrics(runtime::TokioCurrentThread) // Use the chosen runtime
        .with_exporter(metrics_exporter)
        .build()
        .map_err(|e| error!(error = %e, "Failed to initialize OTLP metrics pipeline"))
        .ok()?;

    otel_global::set_meter_provider(meter_provider.clone()); // Set the global meter provider
    let meter = otel_global::meter_provider().meter("intercept-bounce"); // Get a meter instance
    info!("OpenTelemetry exporter initialized successfully.");
    Some((meter_provider, tracer, meter)) // Return the meter as well
}

// Initialize tracing subscriber
fn init_tracing(cfg: &Config) -> Option<Meter> {
    // Return Option<Meter>
    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr) // Explicitly write to stderr
        .with_target(cfg.verbose) // Show module path only if verbose
        .with_level(true);

    let filter = EnvFilter::try_new(&cfg.log_filter).unwrap_or_else(|e| {
        eprintln!("Warning: Invalid RUST_LOG '{}': {}", cfg.log_filter, e);
        EnvFilter::new("intercept_bounce=info") // Fallback
    });

    // --- Build Subscriber ---
    let registry_base = tracing_subscriber::registry().with(fmt_layer).with(filter);

    // --- OTLP Layer (if configured) ---
    let otel_meter = if let Some((_meter_provider, tracer, meter)) = init_otel(cfg) {
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        // Initialize with the OTLP layer included
        registry_base.with(otel_layer).init();
        Some(meter) // Return the meter for passing to logger
    } else {
        // Initialize without the OTLP layer
        registry_base.init();
        None
    };
    // registry.init(); // Initialization is now conditional above

    info!(
        version = env!("CARGO_PKG_VERSION"),
        // Use option_env! for git sha to avoid build errors outside git repo
        git_sha = option_env!("VERGEN_GIT_SHA_SHORT").unwrap_or("unknown"),
        build_ts = env!("VERGEN_BUILD_TIMESTAMP"),
        "intercept-bounce starting"
    );

    info!(debounce = %util::format_duration(cfg.debounce_time()),
        near_miss = %util::format_duration(cfg.near_miss_threshold()),
        log_interval = %util::format_duration(cfg.log_interval()),
        log_all = cfg.log_all_events,
        log_bounces = cfg.log_bounces,
        stats_json = cfg.stats_json,
        verbose = cfg.verbose,
        log_filter = %cfg.log_filter,
        otel_endpoint = %cfg.otel_endpoint.as_deref().unwrap_or("<None>"), // Log OTLP endpoint
        "Configuration loaded");

    otel_meter // Return the meter from init_tracing
}

fn main() -> io::Result<()> {
    // Early parse to get config for tracing setup
    let args = cli::parse_args();
    let cfg = Arc::new(Config::from(&args));

    // Initialize tracing as early as possible and get the meter if initialized
    let otel_meter = init_tracing(&cfg);

    // Now use tracing for subsequent messages
    // Don't log the whole config struct at debug, use the info log below
    // debug!("Arguments parsed and config created: {:?}", cfg);

    if args.list_devices {
        info!("Scanning input devices (requires read access to /dev/input/event*)...");
        match list_input_devices() {
            Ok(_) => {
                info!("Device listing complete. Exiting.");
            }
            Err(e) => {
                error!("Error listing devices: {}", e);
                info!("Exiting due to device listing error.");
                exit(2);
            }
        }
        return Ok(());
    }

    set_high_priority();

    debug!("Setting up shared state...");
    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new()));
    let final_stats_printed = Arc::new(AtomicBool::new(false));
    let main_running = Arc::new(AtomicBool::new(true));
    let logger_running = Arc::new(AtomicBool::new(true));
    debug!("Shared state initialized");

    debug!("Creating bounded channel for logger communication...");
    let (log_sender, log_receiver): (Sender<LogMessage>, Receiver<LogMessage>) =
        bounded(LOGGER_QUEUE_CAPACITY); // Keep bounded for now
    debug!(capacity = LOGGER_QUEUE_CAPACITY, "Channel created");

    debug!("Spawning logger thread...");
    let logger_cfg = Arc::clone(&cfg);
    let logger_running_clone_for_logger = Arc::clone(&logger_running);
    let logger_otel_meter = otel_meter.clone(); // Clone the meter for the logger thread
    let logger_handle: JoinHandle<StatsCollector> = thread::spawn(move || {
        let mut logger = Logger::new(
            log_receiver, // Pass Receiver directly
            logger_running_clone_for_logger,
            logger_cfg,
            logger_otel_meter, // Pass the meter
        );
        logger.run()
    });
    debug!("Logger thread spawned");

    debug!("Setting up signal handling thread...");
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let main_running_clone = Arc::clone(&main_running);
    let logger_running_clone_for_signal = Arc::clone(&logger_running);
    let final_stats_printed_clone = Arc::clone(&final_stats_printed);
    // No need to clone verbose, use cfg directly if needed, or just log unconditionally

    thread::spawn(move || {
        debug!(target: "signal_handler", "Signal handling thread started.");
        if let Some(sig) = signals.forever().next() {
            // Use warn level for signal received, as it's an external event causing shutdown
            warn!(signal = sig, "Received signal, initiating shutdown.");
            debug!(target: "signal_handler", "Setting main_running flag to false.");
            main_running_clone.store(false, Ordering::SeqCst);
            debug!(target: "signal_handler", "Setting logger_running flag to false.");
            logger_running_clone_for_signal.store(false, Ordering::SeqCst);
            debug!(target: "signal_handler", "Setting final_stats_printed flag to true.");
            final_stats_printed_clone.store(true, Ordering::SeqCst);
            debug!(target: "signal_handler", "Signal handling complete. Thread exiting");
        }
    });
    debug!("Signal handling thread spawned");

    info!("Entering main event loop");
    let stdin_fd = io::stdin().as_raw_fd();
    info!(stdin_fd, "Reading from standard input");
    let stdout_fd = io::stdout().as_raw_fd();
    // Log Duration directly using Display impl via humantime
    debug!(stdout_fd, debounce = %util::format_duration(cfg.debounce_time()), "Using stdout FD and debounce time.");

    let mut main_state = MainState {
        log_sender, // Use the alias
        warned_about_dropping: false,
        currently_dropping: false,
        total_dropped_log_messages: 0,
    };
    debug!("MainState initialized");

    let check_interval = Duration::from_millis(100); // Used for sleep on EINTR
    debug!(?check_interval, "Using check interval for EINTR sleep");

    // --- OTLP Metrics Setup (in main thread) ---
    // Get counters only if the meter was successfully initialized
    let events_processed_counter = otel_meter.as_ref().map(|m| {
        m.u64_counter("events.processed")
            .with_description("Total input events processed")
            .init()
    });
    let events_passed_counter = otel_meter.as_ref().map(|m| {
        m.u64_counter("events.passed")
            .with_description("Input events passed through the filter")
            .init()
    });
    let events_dropped_counter = otel_meter.as_ref().map(|m| {
        m.u64_counter("events.dropped")
            .with_description("Input events dropped (bounced)")
            .init()
    });
    let log_messages_dropped_counter = otel_meter.as_ref().map(|m| {
        m.u64_counter("log.messages.dropped")
            .with_description("Log messages dropped due to channel backpressure")
            .init()
    });

    // Add an instrumented span around the main loop
    // Refactored to accept context structs instead of individual arguments
    #[instrument(name="main_event_loop", skip_all, fields(otel.kind = "consumer"))]
    fn run_main_loop(
        ctx: &MainLoopContext, // Use the context struct
        main_state: &mut MainState,
        otel_counters: &OtelCounters, // Use the counters struct
    ) {
        while ctx.main_running.load(Ordering::SeqCst) {
            trace!("Main loop iteration: checking running flag (true)");
            trace!("Attempting to read event from stdin...");

            match read_event_raw(ctx.stdin_fd) {
                Ok(Some(ev)) => {
                    let event_us = event_microseconds(&ev);
                    trace!(ev.type_, ev.code, ev.value, event_us, "Read event");

                    // Increment OTLP processed counter
                    if let Some(counter) = &otel_counters.events_processed { // Use otel_counters
                        counter.add(1, &[]);
                    }

                    trace!("Locking BounceFilter mutex...");
                    let (is_bounce, diff_us, last_passed_us) = {
                        match ctx.bounce_filter.lock() { // Use ctx
                            Ok(mut filter) => {
                                trace!("BounceFilter mutex locked successfully.");
                                let result = filter.check_event(&ev, ctx.cfg.debounce_time()); // Use ctx
                                trace!(?result, "BounceFilter check_event returned");
                                result
                            }
                            Err(poisoned) => {
                                // Use error level for poisoned mutex
                                error!("FATAL: BounceFilter mutex poisoned in main event loop.");
                                let mut filter = poisoned.into_inner();
                                let result = filter.check_event(&ev, ctx.cfg.debounce_time()); // Use ctx
                                trace!(?result, "BounceFilter check_event (poisoned) returned");
                                result
                            }
                        }
                    };
                    trace!("BounceFilter mutex unlocked");

                    let event_info = EventInfo {
                        event: ev, // Cannot log event directly as it doesn't impl Debug
                        event_us,
                        is_bounce,
                        diff_us,
                        last_passed_us,
                    };
                    // Log EventInfo fields individually at trace level
                    trace!(event_type = event_info.event.type_,
                       event_code = event_info.event.code,
                       event_value = event_info.event.value,
                       event_us = event_info.event_us,
                       is_bounce = event_info.is_bounce,
                       diff_us = ?event_info.diff_us, // Use ? for Option<Debug>
                       last_passed_us = ?event_info.last_passed_us,
                       "Prepared EventInfo for logger");

                    trace!("Attempting to send EventInfo to logger channel...");
                    match main_state
                        .log_sender
                        .try_send(LogMessage::Event(event_info))
                    {
                        // Use try_send directly
                        Ok(_) => {
                            trace!("Successfully sent EventInfo to logger");
                            if main_state.currently_dropping {
                                // Use info level when resuming logging
                                info!("Logger channel caught up, resuming logging");
                                main_state.currently_dropping = false;
                            }
                        }
                        Err(TrySendError::Full(_)) => {
                            // Handle Full directly
                            main_state.total_dropped_log_messages += 1;
                            // Increment OTLP dropped log message counter if available
                            if let Some(counter) = &otel_counters.log_messages_dropped {
                                counter.add(1, &[]);
                            }
                            if !main_state.warned_about_dropping {
                                // Use warn level for dropping logs
                                warn!("Logger channel full, dropping log messages to maintain performance");
                                main_state.warned_about_dropping = true;
                                main_state.currently_dropping = true;
                            }
                            trace!(
                                total_dropped = main_state.total_dropped_log_messages,
                                "Logger channel full. Dropped log message"
                            );
                        }
                        Err(TrySendError::Disconnected(_)) => {
                            // Handle Disconnected directly
                            // Error level for unexpected disconnect
                            error!("Logger channel disconnected unexpectedly");
                            debug!("Setting main_running flag to false due to logger channel disconnect");
                            ctx.main_running.store(false, Ordering::SeqCst); // Use ctx
                            debug!("Breaking main loop due to logger channel disconnect");
                            break;
                        }
                    }
                    // Removed conditional send block

                    if !is_bounce {
                        trace!("Event passed filter. Attempting to write to stdout...");
                        // Increment OTLP passed counter
                        if let Some(counter) = &otel_counters.events_passed {
                            counter.add(1, &[]);
                        }

                        if let Err(e) = write_event_raw(ctx.stdout_fd, &ev) { // Use ctx
                            if e.kind() == ErrorKind::BrokenPipe {
                                // Info level for broken pipe is appropriate
                                info!("Output pipe broken, exiting");
                                debug!("Setting main_running flag to false due to BrokenPipe");
                                ctx.main_running.store(false, Ordering::SeqCst); // Use ctx
                                debug!("Breaking main loop due to BrokenPipe");
                                break;
                            } else {
                                // Error level for other write errors
                                error!(error = %e, "Error writing output event");
                                debug!("Setting main_running flag to false due to write error");
                                ctx.main_running.store(false, Ordering::SeqCst); // Use ctx
                                debug!("Breaking main loop due to write error");
                                break;
                            }
                        } else {
                            trace!("Successfully wrote event to stdout");
                        }
                    } else {
                        trace!("Event dropped by filter. Not writing to stdout");
                        // Increment OTLP dropped counter
                        if let Some(counter) = &otel_counters.events_dropped {
                            counter.add(1, &[]);
                        }
                    }
                }
                Ok(None) => {
                    // Info level for clean EOF
                    info!("Received clean EOF on stdin");
                    debug!("Setting main_running flag to false due to EOF");
                    ctx.main_running.store(false, Ordering::SeqCst); // Use ctx
                    debug!("Breaking main loop due to EOF");
                    break;
                }
                Err(e) => {
                    if e.kind() == ErrorKind::Interrupted {
                        // Debug level for EINTR is fine
                        debug!("Read interrupted by signal (EINTR)");
                        trace!(
                            "Sleeping for {:?} before re-checking running flag.",
                            ctx.check_interval // Use ctx
                        );
                        thread::sleep(ctx.check_interval); // Use ctx
                        trace!("Checking main_running flag after EINTR sleep...");
                        if !ctx.main_running.load(Ordering::SeqCst) { // Use ctx
                            debug!("main_running is false after EINTR. Breaking loop");
                            break;
                        }
                        trace!("main_running is still true after EINTR. Continuing read loop");
                        continue; // Continue loop after EINTR
                    }
                    // Error level for other read errors
                    error!(error = %e, "Error reading input event");
                    debug!("Setting main_running flag to false due to read error");
                    ctx.main_running.store(false, Ordering::SeqCst); // Use ctx
                    debug!("Breaking main loop due to read error");
                    break;
                }
            }
        }
    } // End of run_main_loop function

    // Create context structs
    let main_loop_context = MainLoopContext {
        main_running: &main_running,
        stdin_fd,
        stdout_fd,
        bounce_filter: &bounce_filter,
        cfg: &cfg,
        check_interval,
    };

    let otel_counters = OtelCounters {
        events_processed: events_processed_counter,
        events_passed: events_passed_counter,
        events_dropped: events_dropped_counter,
        log_messages_dropped: log_messages_dropped_counter,
    };

    // Call the instrumented function with the context structs
    run_main_loop(&main_loop_context, &mut main_state, &otel_counters);

    info!("Main event loop finished");

    debug!("Starting shutdown process");
    drop(main_state.log_sender); // Drop sender directly
    debug!("log_sender dropped");

    debug!("Waiting for logger thread to join...");
    let final_stats = match logger_handle.join() {
        Ok(stats) => {
            debug!("Logger thread joined successfully");
            stats
        }
        Err(e) => {
            // Error level for thread panic
            error!(panic_info = ?e, "Logger thread panicked");
            debug!("Logger thread panicked. Returning default stats");
            StatsCollector::with_capacity() // Return empty stats
        }
    };
    debug!("Logger thread joined. Final stats collected");

    debug!("Checking final_stats_printed flag before printing");
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
        debug!("Final stats flag was not set. Proceeding to print final stats");
        let runtime_us = {
            match bounce_filter.lock() {
                Ok(filter) => {
                    trace!("BounceFilter mutex locked for runtime calculation");
                    let rt = filter.get_runtime_us();
                    trace!(?rt, "BounceFilter runtime_us");
                    rt
                }
                Err(_) => {
                    // Warn level for poisoned mutex during final calculation
                    warn!("BounceFilter mutex poisoned during final runtime calculation");
                    trace!("BounceFilter mutex poisoned. Cannot get runtime");
                    None
                }
            }
        };
        trace!("BounceFilter mutex unlocked after runtime calculation");

        if cfg.stats_json {
            debug!("Printing final stats in JSON format");
            // Use a dedicated tracing event for stats output
            info!(target: "stats", stats_kind = "cumulative", format = "json", "Emitting final statistics");
            final_stats.print_stats_json(
                &cfg, // Remove explicit auto-deref
                runtime_us,
                "Cumulative",             // Report type
                &mut io::stderr().lock(), // Write directly to stderr
            );
            debug!("Finished printing final stats in JSON format");
        } else {
            debug!("Printing final stats in human-readable format");
            // Use a dedicated tracing event for stats output
            info!(target: "stats", stats_kind = "cumulative", format = "human", "Emitting final statistics");
            final_stats.print_stats_to_stderr(&cfg, "Cumulative"); // Remove explicit auto-deref
            debug!("Finished printing main stats block");
            if let Some(rt) = runtime_us {
                // Use info level for final runtime print, format as duration
                info!(runtime = %util::format_duration(Duration::from_micros(rt)), "Total Runtime");
                debug!("Finished printing runtime");
            } else {
                debug!("Runtime not available");
            }
        }
        if main_state.total_dropped_log_messages > 0 {
            // Warn level for dropped log messages
            warn!(
                count = main_state.total_dropped_log_messages,
                "Total log messages dropped due to logger backpressure"
            );
            debug!("Finished printing dropped log message count");
        } else {
            debug!("No log messages were dropped");
        }
    } else {
        debug!("Final statistics flag was already set (expected on signal). Skipping final stats print in main");
    }

    // --- OTLP Shutdown ---
    otel_global::shutdown_tracer_provider(); // Cleanly shutdown OTLP tracer
                                             // Meter provider shutdown is typically handled by dropping the provider instance.
                                             // We don't store it directly in main, so removing the explicit call.
                                             // otel_global::shutdown_meter_provider();
    info!("Application exiting successfully");
    Ok(())
}
