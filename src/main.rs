// Orchestrates command-line parsing, thread setup, the main event loop,
// signal handling, and final shutdown/stats reporting.

use crossbeam_channel::{bounded, Receiver, Sender, TrySendError};
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::io::{self, ErrorKind};
use std::os::fd::RawFd;
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
use logger::{LogMessage, Logger}; // Removed EventInfo from here
use tracing::{debug, error, info, instrument, trace, warn};

// --- OTLP Imports ---
use opentelemetry::global as otel_global;
use opentelemetry::metrics::{Meter, MeterProvider as _};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{metrics::SdkMeterProvider, runtime, trace as sdktrace, Resource};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// Capacity for the channel between the main event loop and the logger thread.
const LOGGER_QUEUE_CAPACITY: usize = 1024;

/// State for the main processing thread.
struct MainState {
    log_sender: Sender<LogMessage>,
    warned_about_dropping: bool,
    currently_dropping: bool,
    total_dropped_log_messages: u64,
}

/// Context information passed to the main event loop.
struct MainLoopContext<'a> {
    main_running: &'a Arc<AtomicBool>,
    stdin_fd: RawFd,
    stdout_fd: RawFd,
    bounce_filter: &'a Arc<Mutex<BounceFilter>>,
    cfg: &'a Arc<Config>,
    check_interval: Duration,
}

/// Optional OpenTelemetry counters used in the main loop.
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
        .install_batch(runtime::TokioCurrentThread)
        .map_err(|e| error!(error = %e, "Failed to initialize OTLP trace pipeline"))
        .ok()?;

    // --- Metrics Pipeline ---
    let metrics_exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(otel_endpoint);
    let meter_provider = opentelemetry_otlp::new_pipeline()
        .metrics(runtime::TokioCurrentThread)
        .with_exporter(metrics_exporter)
        .build()
        .map_err(|e| error!(error = %e, "Failed to initialize OTLP metrics pipeline"))
        .ok()?;

    otel_global::set_meter_provider(meter_provider.clone());
    let meter = otel_global::meter_provider().meter("intercept-bounce");
    info!("OpenTelemetry exporter initialized successfully.");
    Some((meter_provider, tracer, meter))
}

/// Initialize tracing subscriber (fmt layer + optional OTLP layer).
/// Returns the OTLP Meter if OTLP is configured and initialized successfully.
fn init_tracing(cfg: &Config) -> Option<Meter> {
    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(cfg.verbose)
        .with_level(true);

    let filter = EnvFilter::try_new(&cfg.log_filter).unwrap_or_else(|e| {
        eprintln!("Warning: Invalid RUST_LOG '{}': {}", cfg.log_filter, e);
        EnvFilter::new("intercept_bounce=info") // Default filter on parse error
    });

    // Base subscriber registry
    let registry_base = tracing_subscriber::registry().with(fmt_layer).with(filter);

    // Conditionally add OTLP layer and initialize the subscriber
    let otel_meter = if let Some((_meter_provider, tracer, meter)) = init_otel(cfg) {
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
        registry_base.with(otel_layer).init();
        Some(meter) // OTLP initialized, return the meter
    } else {
        registry_base.init(); // Initialize without OTLP
        None
    };

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
        otel_endpoint = %cfg.otel_endpoint.as_deref().unwrap_or("<None>"),
        "Configuration loaded");

    otel_meter
}

fn main() -> io::Result<()> {
    // Parse args early to configure tracing based on verbosity/log settings.
    let args = cli::parse_args();
    let cfg = Arc::new(Config::from(&args));

    // Initialize tracing (potentially including OTLP).
    let otel_meter = init_tracing(&cfg);

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
        bounded(LOGGER_QUEUE_CAPACITY);
    debug!(capacity = LOGGER_QUEUE_CAPACITY, "Channel created");

    debug!("Spawning logger thread...");
    let logger_cfg = Arc::clone(&cfg);
    let logger_running_clone_for_logger = Arc::clone(&logger_running);
    let logger_otel_meter = otel_meter.clone();
    let logger_handle: JoinHandle<StatsCollector> = thread::spawn(move || {
        let mut logger = Logger::new(
            log_receiver,
            logger_running_clone_for_logger,
            logger_cfg,
            logger_otel_meter,
        );
        logger.run()
    });
    debug!("Logger thread spawned");

    debug!("Setting up signal handling thread...");
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let main_running_clone = Arc::clone(&main_running);
    let logger_running_clone_for_signal = Arc::clone(&logger_running);
    let final_stats_printed_clone = Arc::clone(&final_stats_printed);

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
    debug!(stdout_fd, debounce = %util::format_duration(cfg.debounce_time()), "Using stdout FD and debounce time.");

    let mut main_state = MainState {
        log_sender,
        warned_about_dropping: false,
        currently_dropping: false,
        total_dropped_log_messages: 0,
    };
    debug!("MainState initialized");

    let check_interval = Duration::from_millis(100); // Interval to sleep on EINTR
    debug!(?check_interval, "Using check interval for EINTR sleep");

    // --- OTLP Metrics Setup (in main thread) ---
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

    // Instrument the main loop for tracing.
    #[instrument(name="main_event_loop", skip_all, fields(otel.kind = "consumer"))]
    fn run_main_loop(
        ctx: &MainLoopContext,
        main_state: &mut MainState,
        otel_counters: &OtelCounters,
    ) {
        while ctx.main_running.load(Ordering::SeqCst) {
            trace!("Main loop iteration: checking running flag");
            trace!("Attempting to read event from stdin...");

            match read_event_raw(ctx.stdin_fd) {
                Ok(Some(ev)) => {
                    let event_us = event_microseconds(&ev);
                    trace!(ev.type_, ev.code, ev.value, event_us, "Read event");

                    // Increment OTLP processed counter if available.
                    if let Some(counter) = &otel_counters.events_processed {
                        counter.add(1, &[]);
                    }

                    trace!("Locking BounceFilter mutex...");
                    let event_info = {
                        match ctx.bounce_filter.lock() {
                            Ok(mut filter) => {
                                trace!("BounceFilter mutex locked successfully.");
                                let info = filter.check_event(&ev, ctx.cfg.debounce_time());
                                // Log individual fields instead of the whole struct using `?`
                                trace!(event_us = info.event_us, is_bounce = info.is_bounce, diff_us = ?info.diff_us, last_passed_us = ?info.last_passed_us, "BounceFilter check_event returned");
                                info
                            }
                            Err(poisoned) => {
                                error!("FATAL: BounceFilter mutex poisoned in main event loop.");
                                let mut filter = poisoned.into_inner();
                                let info = filter.check_event(&ev, ctx.cfg.debounce_time());
                                // Log individual fields instead of the whole struct using `?`
                                trace!(event_us = info.event_us, is_bounce = info.is_bounce, diff_us = ?info.diff_us, last_passed_us = ?info.last_passed_us, "BounceFilter check_event (poisoned) returned");
                                info
                            }
                        }
                    };
                    trace!("BounceFilter mutex unlocked");

                    // Extract the event needed for stdout *before* event_info is moved.
                    // input_event implements Copy, so this is cheap.
                    let event_to_write = event_info.event;

                    // event_info is now returned directly from check_event
                    trace!(event_type = event_info.event.type_,
                       event_code = event_info.event.code,
                       event_value = event_info.event.value,
                       event_us = event_info.event_us,
                       is_bounce = event_info.is_bounce,
                       diff_us = ?event_info.diff_us,
                       last_passed_us = ?event_info.last_passed_us,
                       "Prepared EventInfo for logger");

                    trace!("Attempting to send EventInfo to logger channel...");
                    match main_state
                        .log_sender
                        .try_send(LogMessage::Event(event_info))
                    {
                        Ok(_) => {
                            trace!("Successfully sent EventInfo to logger");
                            if main_state.currently_dropping {
                                info!("Logger channel caught up, resuming logging");
                                main_state.currently_dropping = false;
                            }
                        }
                        Err(TrySendError::Full(_)) => {
                            main_state.total_dropped_log_messages += 1;
                            if let Some(counter) = &otel_counters.log_messages_dropped {
                                counter.add(1, &[]);
                            }
                            if !main_state.warned_about_dropping {
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
                            error!("Logger channel disconnected unexpectedly");
                            debug!("Setting main_running flag to false due to logger channel disconnect");
                            ctx.main_running.store(false, Ordering::SeqCst);
                            debug!("Breaking main loop due to logger channel disconnect");
                            break;
                        }
                    }

                    if !event_info.is_bounce {
                        trace!("Event passed filter. Attempting to write to stdout...");
                        if let Some(counter) = &otel_counters.events_passed {
                            counter.add(1, &[]);
                        }

                        // Use the extracted event_to_write
                        if let Err(e) = write_event_raw(ctx.stdout_fd, &event_to_write) {
                            if e.kind() == ErrorKind::BrokenPipe {
                                info!("Output pipe broken, exiting");
                                debug!("Setting main_running flag to false due to BrokenPipe");
                                ctx.main_running.store(false, Ordering::SeqCst);
                                debug!("Breaking main loop due to BrokenPipe");
                                break;
                            } else {
                                error!(error = %e, "Error writing output event");
                                debug!("Setting main_running flag to false due to write error");
                                ctx.main_running.store(false, Ordering::SeqCst);
                                debug!("Breaking main loop due to write error");
                                break;
                            }
                        } else {
                            trace!("Successfully wrote event to stdout");
                        }
                    } else {
                        trace!("Event dropped by filter. Not writing to stdout");
                        if let Some(counter) = &otel_counters.events_dropped {
                            counter.add(1, &[]);
                        }
                    }
                }
                Ok(None) => {
                    info!("Received clean EOF on stdin");
                    debug!("Setting main_running flag to false due to EOF");
                    ctx.main_running.store(false, Ordering::SeqCst);
                    debug!("Breaking main loop due to EOF");
                    break;
                }
                Err(e) => {
                    if e.kind() == ErrorKind::Interrupted {
                        debug!("Read interrupted by signal (EINTR)");
                        trace!(
                            "Sleeping for {:?} before re-checking running flag.",
                            ctx.check_interval
                        );
                        thread::sleep(ctx.check_interval);
                        trace!("Checking main_running flag after EINTR sleep...");
                        if !ctx.main_running.load(Ordering::SeqCst) {
                            debug!("main_running is false after EINTR. Breaking loop");
                            break;
                        }
                        trace!("main_running is still true after EINTR. Continuing read loop");
                        continue;
                    }
                    error!(error = %e, "Error reading input event");
                    debug!("Setting main_running flag to false due to read error");
                    ctx.main_running.store(false, Ordering::SeqCst);
                    debug!("Breaking main loop due to read error");
                    break;
                }
            }
        }
    }

    // Group arguments for the main loop function.
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

    // Run the main event processing loop.
    run_main_loop(&main_loop_context, &mut main_state, &otel_counters);

    info!("Main event loop finished");

    debug!("Starting shutdown process");
    drop(main_state.log_sender);
    debug!("log_sender dropped");

    debug!("Waiting for logger thread to join...");
    let final_stats = match logger_handle.join() {
        Ok(stats) => {
            debug!("Logger thread joined successfully");
            stats
        }
        Err(e) => {
            error!(panic_info = ?e, "Logger thread panicked");
            debug!("Logger thread panicked. Returning default stats");
            StatsCollector::with_capacity() // Return empty stats on panic
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
                    warn!("BounceFilter mutex poisoned during final runtime calculation");
                    trace!("BounceFilter mutex poisoned. Cannot get runtime");
                    None
                }
            }
        };
        trace!("BounceFilter mutex unlocked after runtime calculation");

        if cfg.stats_json {
            debug!("Printing final stats in JSON format");
            info!(target: "stats", stats_kind = "cumulative", format = "json", "Emitting final statistics");
            final_stats.print_stats_json(&cfg, runtime_us, "Cumulative", &mut io::stderr().lock());
            debug!("Finished printing final stats in JSON format");
        } else {
            debug!("Printing final stats in human-readable format");
            info!(target: "stats", stats_kind = "cumulative", format = "human", "Emitting final statistics");
            final_stats.print_stats_to_stderr(&cfg, "Cumulative");
            debug!("Finished printing main stats block");
            if let Some(rt) = runtime_us {
                info!(runtime = %util::format_duration(Duration::from_micros(rt)), "Total Runtime");
                debug!("Finished printing runtime");
            } else {
                debug!("Runtime not available");
            }
        }
        if main_state.total_dropped_log_messages > 0 {
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
    otel_global::shutdown_tracer_provider();
    // Meter provider shutdown is handled by dropping the provider instance.
    info!("Application exiting successfully");
    Ok(())
}
