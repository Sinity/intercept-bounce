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
use intercept_bounce::event;
use intercept_bounce::filter::stats::StatsCollector;
use intercept_bounce::filter::BounceFilter;
use intercept_bounce::logger;
use intercept_bounce::telemetry::init_tracing;
use intercept_bounce::{cli, config::Config, util};
use logger::{LogMessage, Logger};
use tracing::{debug, error, info, instrument, trace, warn};

use opentelemetry::global as otel_global;

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

/// Represents reasons why the main event loop might terminate prematurely.
#[derive(Debug)]
enum MainLoopError {
    LoggerDisconnected,
    StdoutBrokenPipe,
    StdoutWriteError(io::Error),
    StdinReadError(io::Error),
}

impl std::fmt::Display for MainLoopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MainLoopError::LoggerDisconnected => write!(f, "Logger channel disconnected"),
            MainLoopError::StdoutBrokenPipe => write!(f, "Stdout pipe broken"), // Already simple
            MainLoopError::StdoutWriteError(e) => write!(f, "Stdout write error: {e}"),
            MainLoopError::StdinReadError(e) => write!(f, "Stdin read error: {e}"),
        }
    }
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
                io::Error::last_os_error() // Keep as is, error needs formatting
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

/// Sets the main and logger running flags to false and logs the shutdown reason.
fn trigger_shutdown(
    reason: &str,
    main_running: &Arc<AtomicBool>,
    logger_running: &Arc<AtomicBool>,
) {
    warn!(reason, "Initiating shutdown."); // Use warn level for shutdown trigger
    main_running.store(false, Ordering::SeqCst);
    logger_running.store(false, Ordering::SeqCst);
}

fn main() -> io::Result<()> {
    let args = cli::parse_args();
    let cfg = Arc::new(Config::from(&args));
    let otel_meter = init_tracing(&cfg);

    if args.list_devices {
        info!("Scanning input devices (requires read access to /dev/input/event*)...");
        match list_input_devices() {
            Ok(_) => {
                info!("Device listing complete. Exiting.");
            }
            Err(e) => {
                error!("Error listing devices: {e}");
                info!("Exiting due to device listing error.");
                exit(2);
            }
        }
        return Ok(());
    }

    set_high_priority();

    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new()));
    let final_stats_printed = Arc::new(AtomicBool::new(false));
    let main_running = Arc::new(AtomicBool::new(true));
    let logger_running = Arc::new(AtomicBool::new(true));

    let (log_sender, log_receiver): (Sender<LogMessage>, Receiver<LogMessage>) =
        bounded(LOGGER_QUEUE_CAPACITY);
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

    // --- Signal Handling Thread ---
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let main_running_signal = Arc::clone(&main_running);
    let logger_running_signal = Arc::clone(&logger_running);
    let final_stats_printed_signal = Arc::clone(&final_stats_printed);
    thread::spawn(move || {
        if let Some(sig) = signals.forever().next() {
            // `sig` is used in format string
            let reason = format!("Received signal {sig}");
            // Ensure final stats are printed by the signal handler if it triggers shutdown.
            final_stats_printed_signal.store(true, Ordering::SeqCst);
            trigger_shutdown(&reason, &main_running_signal, &logger_running_signal);
        }
    });

    info!("Starting main event loop");
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

    let check_interval = Duration::from_millis(100); // Interval to sleep on EINTR

    // --- OTLP Metrics Setup ---
    let otel_counters = OtelCounters {
        events_processed: otel_meter.as_ref().map(|m| {
            m.u64_counter("events.processed")
                .with_description("Total input events processed")
                .init()
        }),
        events_passed: otel_meter.as_ref().map(|m| {
            m.u64_counter("events.passed")
                .with_description("Input events passed through the filter")
                .init()
        }),
        events_dropped: otel_meter.as_ref().map(|m| {
            m.u64_counter("events.dropped")
                .with_description("Input events dropped (bounced)")
                .init()
        }),
        log_messages_dropped: otel_meter.as_ref().map(|m| {
            m.u64_counter("log.messages.dropped")
                .with_description("Log messages dropped due to channel backpressure")
                .init()
        }),
    };

    // Group arguments for the main loop function.
    let main_loop_context = MainLoopContext {
        main_running: &main_running,
        stdin_fd,
        stdout_fd,
        bounce_filter: &bounce_filter,
        cfg: &cfg,
        check_interval,
    };

    // Run the main event processing loop.
    run_main_loop(
        &main_loop_context,
        &mut main_state,
        &otel_counters,
        &logger_running,
    );

    info!("Main event loop finished");

    debug!("Starting shutdown process");
    // Drop the sender to signal the logger thread to finish processing remaining messages.
    drop(main_state.log_sender);

    debug!("Waiting for logger thread to join...");
    let final_stats = match logger_handle.join() {
        Ok(stats) => {
            debug!("Logger thread joined successfully");
            stats
        }
        Err(e) => {
            error!(panic_info = ?e, "Logger thread panicked"); // Keep ?e for debug info
            StatsCollector::with_capacity() // Return empty stats on panic
        }
    };

    // Use an atomic swap on `final_stats_printed`. If this thread successfully
    // changes it from `false` to `true`, it takes responsibility for printing
    // the final stats. This prevents double-printing if the signal handler
    // also triggered shutdown and set the flag.
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
        debug!("Printing final cumulative statistics...");
        let runtime_us = {
            match bounce_filter.lock() {
                Ok(filter) => filter.get_runtime_us(),
                Err(_) => {
                    warn!("BounceFilter mutex poisoned during final runtime calculation");
                    None
                }
            }
        };

        if cfg.stats_json {
            info!(target: "stats", stats_kind = "cumulative", format = "json", "Emitting final statistics");
            final_stats.print_stats_json(&cfg, runtime_us, "Cumulative", &mut io::stderr().lock());
        } else {
            info!(target: "stats", stats_kind = "cumulative", format = "human", "Emitting final statistics");
            final_stats.print_stats_to_stderr(&cfg, "Cumulative");
            if let Some(rt) = runtime_us {
                info!(runtime = %util::format_duration(Duration::from_micros(rt)), "Total Runtime");
                // Keep %util::...
            }
        }
        if main_state.total_dropped_log_messages > 0 {
            warn!(
                count = main_state.total_dropped_log_messages,
                "Total log messages dropped due to logger backpressure"
            );
        }
    } else {
        debug!("Final statistics already printed or handled by signal handler.");
    }

    // --- OTLP Shutdown ---
    otel_global::shutdown_tracer_provider();
    // Meter provider shutdown is handled implicitly by dropping the provider instance if it exists.
    info!("Application exiting successfully");
    Ok(())
}

/// Processes a single input event.
/// Handles filtering, logging, and writing passed events to stdout.
/// Returns Ok(()) on success, or a MainLoopError if the loop should terminate.
#[instrument(skip_all, fields(ev.type = ev.type_, ev.code = ev.code, ev.value = ev.value))]
fn process_event(
    ev: &event::input_event,
    ctx: &MainLoopContext,
    main_state: &mut MainState,
    otel_counters: &OtelCounters,
) -> Result<(), MainLoopError> {
    let event_us = event_microseconds(ev);
    trace!(event_us, "Processing event");

    // Increment OTLP processed counter if available.
    if let Some(counter) = &otel_counters.events_processed {
        counter.add(1, &[]);
    }

    let event_info = {
        match ctx.bounce_filter.lock() {
            Ok(mut filter) => {
                let info = filter.check_event(ev, ctx.cfg.debounce_time());
                trace!(is_bounce = info.is_bounce, diff_us = ?info.diff_us, last_passed_us = ?info.last_passed_us, "BounceFilter check_event returned");
                info
            }
            Err(poisoned) => {
                // If the mutex is poisoned, log fatal, but try to continue by recovering the lock.
                error!("FATAL: BounceFilter mutex poisoned in main event loop. Recovering...");
                let mut filter = poisoned.into_inner();
                let info = filter.check_event(ev, ctx.cfg.debounce_time());
                trace!(is_bounce = info.is_bounce, diff_us = ?info.diff_us, last_passed_us = ?info.last_passed_us, "BounceFilter check_event (poisoned) returned");
                info
            }
        }
    };

    // Extract the event and bounce status *before* event_info is moved.
    let event_to_write = event_info.event;
    let is_bounce = event_info.is_bounce;

    // Send event info to logger thread.
    match main_state
        .log_sender
        .try_send(LogMessage::Event(event_info)) // event_info is moved here
    {
        Ok(_) => {
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
                "Dropped log message (channel full)"
            );
        }
        Err(TrySendError::Disconnected(_)) => {
            // Logger thread terminated unexpectedly.
            return Err(MainLoopError::LoggerDisconnected);
        }
    }

    // Write non-bounced events to stdout.
    if !is_bounce {
        trace!("Event passed filter. Writing to stdout...");
        if let Some(counter) = &otel_counters.events_passed {
            counter.add(1, &[]);
        }

        if let Err(e) = write_event_raw(ctx.stdout_fd, &event_to_write) {
            return if e.kind() == ErrorKind::BrokenPipe {
                Err(MainLoopError::StdoutBrokenPipe)
            } else {
                Err(MainLoopError::StdoutWriteError(e))
            };
        }
        trace!("Successfully wrote event to stdout");
    } else {
        trace!("Event dropped by filter (bounce).");
        if let Some(counter) = &otel_counters.events_dropped {
            counter.add(1, &[]);
        }
    }

    Ok(())
}

/// The main event reading and processing loop.
/// Reads events from stdin, processes them using `process_event`,
/// and handles termination signals or errors.
#[instrument(name="main_event_loop", skip_all, fields(otel.kind = "consumer"))]
fn run_main_loop(
    ctx: &MainLoopContext,
    main_state: &mut MainState,
    otel_counters: &OtelCounters,
    logger_running: &Arc<AtomicBool>, // Pass logger_running for trigger_shutdown
) {
    while ctx.main_running.load(Ordering::SeqCst) {
        match read_event_raw(ctx.stdin_fd) {
            Ok(Some(ev)) => {
                // Process the event, handle potential errors that require loop termination.
                if let Err(e) = process_event(&ev, ctx, main_state, otel_counters) {
                    trigger_shutdown(&e.to_string(), ctx.main_running, logger_running);
                    break; // Exit loop on processing error
                }
            }
            Ok(None) => {
                // Clean EOF on stdin.
                trigger_shutdown("EOF received on stdin", ctx.main_running, logger_running);
                break; // Exit loop on EOF
            }
            Err(e) => {
                if e.kind() == ErrorKind::Interrupted {
                    // Interrupted by a signal (e.g., SIGINT/SIGTERM handled by signal thread).
                    // The signal handler should have already set main_running to false.
                    // Sleep briefly and re-check the flag before potentially continuing.
                    trace!("Read interrupted (EINTR), checking running flag...");
                    thread::sleep(ctx.check_interval);
                    if !ctx.main_running.load(Ordering::SeqCst) {
                        trace!("Running flag is false after EINTR. Exiting loop.");
                        break; // Exit if flag was set by signal handler
                    }
                    trace!("Running flag still true after EINTR. Continuing read loop.");
                    continue; // Otherwise, continue reading
                } else {
                    // Other read error.
                    let error = MainLoopError::StdinReadError(e); // `e` used in trigger_shutdown
                    trigger_shutdown(&error.to_string(), ctx.main_running, logger_running);
                    break; // Exit loop on read error
                }
            }
        }
    }
}
