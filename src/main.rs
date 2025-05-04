// Orchestrates command-line parsing, thread setup, the main event loop,
// signal handling, and final shutdown/stats reporting.

use crossbeam_channel::{bounded, Sender, Receiver, TrySendError};
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::io::{self, ErrorKind};
use std::os::unix::io::AsRawFd;
use std::process::exit;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use intercept_bounce::{cli, config::Config, util};
use intercept_bounce::event;
use intercept_bounce::filter;
use intercept_bounce::logger;
use event::{event_microseconds, list_input_devices, read_event_raw, write_event_raw};
use filter::stats::StatsCollector;
use filter::BounceFilter;
use logger::{EventInfo, LogMessage, Logger};
use tracing::{debug, error, info, trace, warn};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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

/// Attempts to set the process priority to the highest level (-20 niceness).
/// Prints a warning if it fails (e.g., due to insufficient permissions).
fn set_high_priority() {
    #[cfg(target_os = "linux")]
    {
        debug!("Attempting to set high process priority (niceness -20)...");
        let res = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -20) };
        if res != 0 {
            warn!("Unable to set process niceness to -20 (requires root or CAP_SYS_NICE). Error: {}", io::Error::last_os_error());
        } else {
            info!("Process priority set to -20 (highest).");
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        info!("set_high_priority is only implemented for Linux.");
    }
}

// Initialize tracing subscriber
fn init_tracing(cfg: &Config) {
    // TODO: Add JSON and OTLP layers later
    let fmt_layer = fmt::layer()
        .with_writer(std::io::stderr) // Explicitly write to stderr
        .with_target(cfg.verbose)     // Show module path only if verbose
        .with_level(true);

    let filter = EnvFilter::try_new(&cfg.log_filter)
        .unwrap_or_else(|e| {
            eprintln!("Warning: Invalid RUST_LOG '{}': {}", cfg.log_filter, e);
            EnvFilter::new("intercept_bounce=info") // Fallback
        });

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(filter)
        .init();

    info!(version = env!("CARGO_PKG_VERSION"),
        // Use option_env! for git sha to avoid build errors outside git repo
        git_sha = option_env!("VERGEN_GIT_SHA_SHORT").unwrap_or("unknown"),
        build_ts = env!("VERGEN_BUILD_TIMESTAMP"),
        "intercept-bounce starting");

    info!(debounce = %util::format_duration(cfg.debounce_time()),
        near_miss = %util::format_duration(cfg.near_miss_threshold()),
        log_interval = %util::format_duration(cfg.log_interval()),
        log_all = cfg.log_all_events,
        log_bounces = cfg.log_bounces,
        stats_json = cfg.stats_json,
        verbose = cfg.verbose,
        log_filter = %cfg.log_filter,
        "Configuration loaded");
}


fn main() -> io::Result<()> {
    // Early parse to get config for tracing setup
    let args = cli::parse_args();
    let cfg = Arc::new(Config::from(&args));

    // Initialize tracing as early as possible
    init_tracing(&cfg);

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
    let (log_sender, log_receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(LOGGER_QUEUE_CAPACITY); // Keep bounded for now
    debug!(capacity = LOGGER_QUEUE_CAPACITY, "Channel created");

    debug!("Spawning logger thread...");
    let logger_cfg = Arc::clone(&cfg);
    let logger_running_clone_for_logger = Arc::clone(&logger_running);
    let logger_handle: JoinHandle<StatsCollector> = thread::spawn(move || {
        let mut logger = Logger::new(
            log_receiver, // Pass Receiver directly
            logger_running_clone_for_logger,
            logger_cfg,
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

    while main_running.load(Ordering::SeqCst) {
        trace!("Main loop iteration: checking running flag (true)");
        trace!("Attempting to read event from stdin...");

        match read_event_raw(stdin_fd) {
            Ok(Some(ev)) => {
                let event_us = event_microseconds(&ev);
                trace!(ev.type_, ev.code, ev.value, event_us, "Read event");

                trace!("Locking BounceFilter mutex...");
                let (is_bounce, diff_us, last_passed_us) = {
                    match bounce_filter.lock() {
                        Ok(mut filter) => {
                            trace!("BounceFilter mutex locked successfully.");
                            let result = filter.check_event(&ev, cfg.debounce_time()); // Use Duration
                            trace!(?result, "BounceFilter check_event returned");
                            result
                        },
                        Err(poisoned) => {
                            // Use error level for poisoned mutex
                            error!("FATAL: BounceFilter mutex poisoned in main event loop.");
                            let mut filter = poisoned.into_inner();
                            let result = filter.check_event(&ev, cfg.debounce_time()); // Use Duration
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
                match main_state.log_sender.try_send(LogMessage::Event(event_info)) { // Use try_send directly
                    Ok(_) => {
                        trace!("Successfully sent EventInfo to logger");
                        if main_state.currently_dropping {
                            // Use info level when resuming logging
                            info!("Logger channel caught up, resuming logging");
                            main_state.currently_dropping = false;
                        }
                    }
                    Err(TrySendError::Full(_)) => { // Handle Full directly
                        main_state.total_dropped_log_messages += 1;
                        if !main_state.warned_about_dropping {
                            // Use warn level for dropping logs
                            warn!("Logger channel full, dropping log messages to maintain performance");
                            main_state.warned_about_dropping = true;
                            main_state.currently_dropping = true;
                        }
                        trace!(total_dropped = main_state.total_dropped_log_messages,
                            "Logger channel full. Dropped log message");
                    }
                    Err(TrySendError::Disconnected(_)) => { // Handle Disconnected directly
                        // Error level for unexpected disconnect
                        error!("Logger channel disconnected unexpectedly");
                        debug!("Setting main_running flag to false due to logger channel disconnect");
                        main_running.store(false, Ordering::SeqCst);
                        debug!("Breaking main loop due to logger channel disconnect");
                        break;
                    }
                }
                // Removed conditional send block


                if !is_bounce {
                    trace!("Event passed filter. Attempting to write to stdout...");
                    if let Err(e) = write_event_raw(stdout_fd, &ev) {
                        if e.kind() == ErrorKind::BrokenPipe {
                            // Info level for broken pipe is appropriate
                            info!("Output pipe broken, exiting");
                            debug!("Setting main_running flag to false due to BrokenPipe");
                            main_running.store(false, Ordering::SeqCst);
                            debug!("Breaking main loop due to BrokenPipe");
                            break;
                        } else {
                            // Error level for other write errors
                            error!(error = %e, "Error writing output event");
                            debug!("Setting main_running flag to false due to write error");
                            main_running.store(false, Ordering::SeqCst);
                            debug!("Breaking main loop due to write error");
                            break;
                        }
                    } else {
                        trace!("Successfully wrote event to stdout");
                    }
                } else {
                    trace!("Event dropped by filter. Not writing to stdout");
                }
            }
            Ok(None) => {
                // Info level for clean EOF
                info!("Received clean EOF on stdin");
                debug!("Setting main_running flag to false due to EOF");
                main_running.store(false, Ordering::SeqCst);
                debug!("Breaking main loop due to EOF");
                break;
            }
            Err(e) => {
                if e.kind() == ErrorKind::Interrupted {
                    // Debug level for EINTR is fine
                    debug!("Read interrupted by signal (EINTR)");
                    trace!("Sleeping for {:?} before re-checking running flag.", check_interval);
                    thread::sleep(check_interval);
                    trace!("Checking main_running flag after EINTR sleep...");
                    if !main_running.load(Ordering::SeqCst) {
                        debug!("main_running is false after EINTR. Breaking loop");
                        break;
                    }
                    trace!("main_running is still true after EINTR. Continuing read loop");
                    continue; // Continue loop after EINTR
                }
                // Error level for other read errors
                error!(error = %e, "Error reading input event");
                debug!("Setting main_running flag to false due to read error");
                main_running.store(false, Ordering::SeqCst);
                debug!("Breaking main loop due to read error");
                break;
            }
        }
    }

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
    } ;
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
                },
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
                &*cfg,
                runtime_us,
                "Cumulative", // Report type
                &mut io::stderr().lock(), // Write directly to stderr
            );
            debug!("Finished printing final stats in JSON format");
        } else {
            debug!("Printing final stats in human-readable format");
            // Use a dedicated tracing event for stats output
            info!(target: "stats", stats_kind = "cumulative", format = "human", "Emitting final statistics");
            final_stats.print_stats_to_stderr(&*cfg, "Cumulative"); // Pass config and type
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
            warn!(count = main_state.total_dropped_log_messages,
                "Total log messages dropped due to logger backpressure");
            debug!("Finished printing dropped log message count");
        } else {
            debug!("No log messages were dropped");
        }
    } else {
        debug!("Final statistics flag was already set (expected on signal). Skipping final stats print in main");
    }

    info!("Application exiting successfully");
    Ok(())
}
