// Main application entry point.
// Orchestrates command-line parsing, thread setup, the main event loop,
// signal handling, and final shutdown/stats reporting.

use colored::*;
use crossbeam_channel::{bounded, Sender, TrySendError, Receiver};
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
use std::time::Duration; // Added Duration for check_interval

// Removed: mod cli; - Now part of the library crate

// Use modules from the library crate
use intercept_bounce::cli;
use intercept_bounce::event;
use intercept_bounce::filter;
mod logger;
mod config;

use config::Config;
use event::{list_input_devices, read_event_raw, write_event_raw, event_microseconds};
use filter::BounceFilter;
use logger::{EventInfo, LogMessage, Logger};
use filter::stats::StatsCollector;

/// Attempts to set the process priority to the highest level (-20 niceness).
/// Prints a warning if it fails (e.g., due to insufficient permissions).
fn set_high_priority(verbose: bool) {
    if verbose { eprintln!("{}", "[MAIN] Attempting to set high process priority...".dimmed()); }
    // Niceness is primarily a Unix concept.
    #[cfg(target_os = "linux")]
    {
        // Use libc to call the setpriority system call.
        // SAFETY: Calling libc::setpriority is unsafe. We provide valid constants.
        let res = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -20) };
        if res != 0 {
                // Failed to set priority. Print a warning to stderr.
                eprintln!(
                    "{}",
                    "[WARN] Unable to set process niceness to -20 (requires root or CAP_SYS_NICE)."
                        .yellow()
                );
            } else {
                // Successfully set priority. Print an info message.
                if verbose { eprintln!("{}", "[INFO] Process priority set to -20 (highest).".dimmed()); }
            }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if verbose { eprintln!("{}", "[INFO] set_high_priority is only implemented for Linux.".dimmed()); }
    }
}

/// State specific to the main processing thread for managing communication
/// with the logger thread and handling log drop warnings.
struct MainState {
    log_sender: Sender<LogMessage>,
    warned_about_dropping: bool, // Have we warned about drops *this session*?
    currently_dropping: bool,    // Are we *currently* dropping messages?
    total_dropped_log_messages: u64, // Track total drops
}

fn main() -> io::Result<()> {
    eprintln!("{}", "[MAIN] Application started.".dimmed());

    // Parse command-line arguments using clap via the library module.
    let args = cli::parse_args();
    let verbose = args.verbose; // Get verbose flag early
    if verbose { eprintln!("{}", format!("[MAIN] Arguments parsed: {:?}", args).dimmed()); }

    // --- Device Listing Mode ---
    // Handle the --list-devices flag separately and exit.
    if args.list_devices {
        eprintln!(
            "{}",
            "Scanning input devices (requires read access to /dev/input/event*)..."
                .on_bright_black()
                .bold()
                .bright_cyan()
        );
        match list_input_devices() {
            Ok(_) => {
                if verbose { eprintln!("{}", "[MAIN] Device listing complete. Exiting.".dimmed()); }
            }
            Err(e) => {
                eprintln!(
                    "{} {}",
                    "Error listing devices:".on_bright_black().red().bold(),
                    e
                );
                eprintln!(
                    "{}",
                    "Note: Listing devices requires read access to /dev/input/event*, typically requiring root privileges."
                        .on_bright_black()
                        .yellow()
                );
                if verbose { eprintln!("{}", "[MAIN] Exiting due to device listing error.".dimmed()); }
                exit(2); // Exit with a specific error code for device listing failure
            }
        }
        return Ok(());
    }

    // Proceed with normal filtering mode
    set_high_priority(verbose); // Attempt to increase priority

    // Unified config struct
    let cfg = Arc::new(Config::from(&args));

    // Shared state setup
    if cfg.verbose { eprintln!("{}", "[MAIN] Setting up shared state (BounceFilter, AtomicBools)...".dimmed()); }
    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new()));
    let final_stats_printed = Arc::new(AtomicBool::new(false));
    let main_running = Arc::new(AtomicBool::new(true)); // Flag for main loop
    let logger_running = Arc::new(AtomicBool::new(true)); // Flag for logger loop
    if cfg.verbose { eprintln!("{}", "[MAIN] Shared state initialized.".dimmed()); }

    // --- Channel for Main -> Logger communication ---
    if cfg.verbose { eprintln!("{}", "[MAIN] Creating bounded channel for logger communication...".dimmed()); }
    let (log_sender, log_receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(1024);
    if cfg.verbose { eprintln!("{}", "[MAIN] Channel created with capacity 1024.".dimmed()); }

    // --- Logger Thread ---
    if cfg.verbose { eprintln!("{}", "[MAIN] Spawning logger thread...".dimmed()); }
    let logger_cfg = Arc::clone(&cfg);
    let logger_running_clone_for_logger = Arc::clone(&logger_running);
    let logger_handle: JoinHandle<StatsCollector> = thread::spawn(move || {
        let mut logger = Logger::new(
            log_receiver,
            logger_running_clone_for_logger,
            logger_cfg,
        );
        let final_stats = logger.run();
        // Use config.verbose for logger thread
        final_stats
    });
    if cfg.verbose { eprintln!("{}", "[MAIN] Logger thread spawned.".dimmed()); }


    // --- Signal Handling Thread ---
    if verbose { eprintln!("{}", "[MAIN] Setting up signal handling thread...".dimmed()); }
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let main_running_clone = Arc::clone(&main_running);
    let logger_running_clone_for_signal = Arc::clone(&logger_running); // Clone logger flag for signal handler
    let final_stats_printed_clone = Arc::clone(&final_stats_printed);
    let verbose_clone = verbose; // Clone verbose for signal thread

    thread::spawn(move || {
        if verbose_clone { eprintln!("{}", "[SIGNAL] Signal handling thread started.".dimmed()); }
        if let Some(sig) = signals.forever().next() {
            eprintln!(
                "\n{} {}",
                "[SIGNAL] Received signal:".on_bright_black().yellow().bold(),
                sig
            );
            // Signal the main loop and logger thread to stop.
            if verbose_clone { eprintln!("{}", "[SIGNAL] Setting main_running flag to false.".dimmed()); }
            main_running_clone.store(false, Ordering::SeqCst);
            if verbose_clone { eprintln!("{}", "[SIGNAL] Setting logger_running flag to false.".dimmed()); }
            logger_running_clone_for_signal.store(false, Ordering::SeqCst); // Signal logger thread directly
            // Set flag to prevent double printing if main loop also exits cleanly.
            if verbose_clone { eprintln!("{}", "[SIGNAL] Setting final_stats_printed flag to true.".dimmed()); }
            final_stats_printed_clone.store(true, Ordering::SeqCst);
            // Note: Actual stats printing now happens after the main loop exits.
            // We just signal termination here.
            if verbose_clone { eprintln!("{}", "[SIGNAL] Signal handling complete. Thread exiting.".dimmed()); }
        }
    });
    if verbose { eprintln!("{}", "[MAIN] Signal handling thread spawned.".dimmed()); }


    // --- Main Event Loop ---
    if verbose { eprintln!("{}", "[MAIN] Entering main event loop.".dimmed()); }
    let stdin_fd = io::stdin().as_raw_fd();
    let stdout_fd = io::stdout().as_raw_fd();
    let debounce_time_us = cfg.debounce_us;
    if cfg.verbose { eprintln!("{}", format!("[MAIN] Using stdin_fd: {}, stdout_fd: {}, debounce_time_us: {}", stdin_fd, stdout_fd, debounce_time_us).dimmed()); }


    // State for managing logger communication backpressure
    let mut main_state = MainState {
        log_sender, // Move the original sender here
        warned_about_dropping: false,
        currently_dropping: false,
        total_dropped_log_messages: 0,
    };
    if cfg.verbose { eprintln!("{}", "[MAIN] MainState initialized.".dimmed()); }


    // How often to check the running flag when read is interrupted.
    let check_interval = Duration::from_millis(100);
    if cfg.verbose { eprintln!("{}", format!("[MAIN] Read check_interval: {:?}", check_interval).dimmed()); }


    while main_running.load(Ordering::SeqCst) {
        if cfg.verbose { eprintln!("{}", "[MAIN] Loop iteration: Checking main_running flag (true).".dimmed()); }
        if cfg.verbose { eprintln!("{}", "[MAIN] Attempting to read event from stdin...".dimmed()); }
        match read_event_raw(stdin_fd) {
            Ok(Some(ev)) => {
                let event_us = event_microseconds(&ev);
                if cfg.verbose { eprintln!("{}", format!("[MAIN] Read event: type={}, code={}, value={}, ts_us={}", ev.type_, ev.code, ev.value, event_us).dimmed()); }

                if cfg.verbose { eprintln!("{}", "[MAIN] Locking BounceFilter mutex...".dimmed()); }
                let (is_bounce, diff_us, last_passed_us) = {
                    // Lock filter only for the check_event call
                    match bounce_filter.lock() {
                        Ok(mut filter) => {
                            if cfg.verbose { eprintln!("{}", "[MAIN] BounceFilter mutex locked successfully.".dimmed()); }
                            let result = filter.check_event(&ev, debounce_time_us);
                            if cfg.verbose { eprintln!("{}", format!("[MAIN] BounceFilter check_event returned: {:?}", result).dimmed()); }
                            result
                        },
                        Err(poisoned) => {
                            eprintln!("{}", "FATAL: BounceFilter mutex poisoned in main event loop.".on_bright_black().red().bold());
                            // Attempt to continue with the poisoned guard, might be inconsistent
                            let mut filter = poisoned.into_inner();
                            let result = filter.check_event(&ev, debounce_time_us);
                            if cfg.verbose { eprintln!("{}", format!("[MAIN] BounceFilter check_event (poisoned) returned: {:?}", result).dimmed()); }
                            result
                        }
                    }
                }; // Mutex unlocked here
                if cfg.verbose { eprintln!("{}", "[MAIN] BounceFilter mutex unlocked.".dimmed()); }


                // Prepare message for logger thread
                let event_info = EventInfo {
                    event: ev, // Pass the original event struct
                    event_us,
                    is_bounce,
                    diff_us,
                    last_passed_us,
                };
                // Cannot print EventInfo directly due to input_event not implementing Debug
                if cfg.verbose {
                    eprintln!("{}", format!(
                        "[MAIN] Prepared EventInfo for logger: type={}, code={}, value={}, event_us={}, is_bounce={}, diff_us={:?}, last_passed_us={:?}",
                        event_info.event.type_, event_info.event.code, event_info.event.value, event_info.event_us, event_info.is_bounce, event_info.diff_us, event_info.last_passed_us
                    ).dimmed());
                }


                // Send event info to logger thread (non-blocking)
                if cfg.verbose { eprintln!("{}", "[MAIN] Attempting to send EventInfo to logger channel...".dimmed()); }
                match main_state.log_sender.try_send(LogMessage::Event(event_info)) {
                    Ok(_) => {
                        if cfg.verbose { eprintln!("{}", "[MAIN] Successfully sent EventInfo to logger.".dimmed()); }
                        if main_state.currently_dropping {
                            eprintln!("{}", "[INFO] Logger channel caught up, resuming logging.".dimmed());
                            main_state.currently_dropping = false;
                        }
                    }
                    Err(TrySendError::Full(_)) => {
                        main_state.total_dropped_log_messages += 1;
                        if !main_state.warned_about_dropping {
                            eprintln!("{}", "[WARN] Logger channel full, dropping log messages to maintain performance.".yellow());
                            main_state.warned_about_dropping = true;
                            main_state.currently_dropping = true;
                        }
                        if cfg.verbose { eprintln!("{}", format!("[MAIN] Logger channel full. Dropped log message. Total dropped: {}", main_state.total_dropped_log_messages).dimmed()); }
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        eprintln!("{}", "[ERROR] Logger channel disconnected unexpectedly.".red());
                        if cfg.verbose { eprintln!("{}", "[MAIN] Setting main_running flag to false due to logger channel disconnect.".dimmed()); }
                        main_running.store(false, Ordering::SeqCst);
                        if cfg.verbose { eprintln!("{}", "[MAIN] Breaking main loop due to logger channel disconnect.".dimmed()); }
                        break;
                    }
                }

                // Write event to stdout if it wasn't a bounce
                if !is_bounce {
                    if cfg.verbose { eprintln!("{}", "[MAIN] Event passed filter. Attempting to write to stdout...".dimmed()); }
                    if let Err(e) = write_event_raw(stdout_fd, &ev) {
                        if e.kind() == ErrorKind::BrokenPipe {
                            eprintln!("{}", "[INFO] Output pipe broken, exiting.".dimmed());
                            if cfg.verbose { eprintln!("{}", "[MAIN] Setting main_running flag to false due to BrokenPipe.".dimmed()); }
                            main_running.store(false, Ordering::SeqCst);
                            if cfg.verbose { eprintln!("{}", "[MAIN] Breaking main loop due to BrokenPipe.".dimmed()); }
                            break;
                        } else {
                            eprintln!(
                                "{} {}",
                                "Error writing output event:".on_bright_black().red().bold(),
                                e
                            );
                            if cfg.verbose { eprintln!("{}", "[MAIN] Setting main_running flag to false due to write error.".dimmed()); }
                            main_running.store(false, Ordering::SeqCst);
                            if cfg.verbose { eprintln!("{}", "[MAIN] Breaking main loop due to write error.".dimmed()); }
                            break;
                        }
                    } else {
                        if cfg.verbose { eprintln!("{}", "[MAIN] Successfully wrote event to stdout.".dimmed()); }
                    }
                } else {
                    if cfg.verbose { eprintln!("{}", "[MAIN] Event dropped by filter. Not writing to stdout.".dimmed()); }
                }
            }
            Ok(None) => {
                // Clean EOF on stdin
                eprintln!("{}", "[MAIN] Received clean EOF on stdin.".dimmed());
                if verbose { eprintln!("{}", "[MAIN] Setting main_running flag to false due to EOF.".dimmed()); }
                main_running.store(false, Ordering::SeqCst); // Signal exit
                if verbose { eprintln!("{}", "[MAIN] Breaking main loop due to EOF.".dimmed()); }
                break; // Exit loop
            }
            Err(e) => {
                // Handle read errors
                if e.kind() == ErrorKind::Interrupted {
                    if verbose { eprintln!("{}", "[MAIN] Read interrupted by signal (EINTR).".dimmed()); }
                    // Interrupted by signal, check running flag
                    // Add a small sleep to avoid tight loop on EINTR if signal handler is slow
                    // or if multiple signals are pending.
                    if verbose { eprintln!("{}", format!("[MAIN] Sleeping for {:?} before re-checking running flag.", check_interval).dimmed()); }
                    thread::sleep(check_interval);
                    if verbose { eprintln!("{}", "[MAIN] Checking main_running flag after EINTR sleep...".dimmed()); }
                    if !main_running.load(Ordering::SeqCst) {
                        if verbose { eprintln!("{}", "[MAIN] main_running is false after EINTR. Breaking loop.".dimmed()); }
                        break; // Exit loop
                    }
                    if verbose { eprintln!("{}", "[MAIN] main_running is still true after EINTR. Continuing read loop.".dimmed()); }
                    continue; // Otherwise, retry read
                }
                eprintln!(
                    "\n{} {}",
                    "Error reading input event:".on_bright_black().red().bold(),
                    e
                );
                if verbose { eprintln!("{}", "[MAIN] Setting main_running flag to false due to read error.".dimmed()); }
                main_running.store(false, Ordering::SeqCst); // Signal exit
                if verbose { eprintln!("{}", "[MAIN] Breaking main loop due to read error.".dimmed()); }
                break; // Exit loop gracefully to allow shutdown
            }
        }
    }

    if cfg.verbose { eprintln!("{}", "[MAIN] Main event loop finished.".dimmed()); }

    // --- Shutdown ---
    if cfg.verbose { eprintln!("{}", "[MAIN] Starting shutdown process.".dimmed()); }
    drop(main_state.log_sender);
    if cfg.verbose { eprintln!("{}", "[MAIN] log_sender dropped.".dimmed()); }

    if cfg.verbose { eprintln!("{}", "[MAIN] Waiting for logger thread to join...".dimmed()); }
    let final_stats = match logger_handle.join() {
        Ok(stats) => {
            if cfg.verbose { eprintln!("{}", "[MAIN] Logger thread joined successfully.".dimmed()); }
            stats
        }
        Err(e) => {
            eprintln!("{} {:?}", "[ERROR] Logger thread panicked:".red().bold(), e);
            if cfg.verbose { eprintln!("{}", "[MAIN] Logger thread panicked. Returning default stats.".dimmed()); }
            StatsCollector::with_capacity()
        }
    };
    if cfg.verbose { eprintln!("{}", "[MAIN] Logger thread joined. Final stats collected.".dimmed()); }

    if cfg.verbose { eprintln!("{}", "[MAIN] Checking final_stats_printed flag before printing.".dimmed()); }
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
        if cfg.verbose { eprintln!("{}", "[MAIN] Final stats flag was not set. Proceeding to print final stats.".dimmed()); }
        let runtime_us = {
            match bounce_filter.lock() {
                Ok(filter) => {
                    if cfg.verbose { eprintln!("{}", "[MAIN] BounceFilter mutex locked for runtime calculation.".dimmed()); }
                    let rt = filter.get_runtime_us();
                    if cfg.verbose { eprintln!("{}", format!("[MAIN] BounceFilter runtime_us: {:?}", rt).dimmed()); }
                    rt
                },
                Err(_) => {
                    eprintln!("{}", "[WARN] BounceFilter mutex poisoned during final runtime calculation.".yellow());
                    if cfg.verbose { eprintln!("{}", "[MAIN] BounceFilter mutex poisoned. Cannot get runtime.".dimmed()); }
                    None
                }
            }
        };
        if cfg.verbose { eprintln!("{}", "[MAIN] BounceFilter mutex unlocked after runtime calculation.".dimmed()); }

        if cfg.stats_json {
            if cfg.verbose { eprintln!("{}", "[MAIN] Printing final stats in JSON format.".dimmed()); }
            final_stats.print_stats_json(
                &cfg,
                runtime_us,
                &mut io::stderr().lock(),
            );
            if cfg.verbose { eprintln!("{}", "[MAIN] Finished printing final stats in JSON format.".dimmed()); }
        } else {
            if cfg.verbose { eprintln!("{}", "[MAIN] Printing final stats in human-readable format.".dimmed()); }
            final_stats.print_stats_to_stderr(&cfg);
            if cfg.verbose { eprintln!("{}", "[MAIN] Finished printing main stats block.".dimmed()); }
            if let Some(rt) = runtime_us {
                eprintln!(
                    "{} {}",
                    "Total Runtime:".on_bright_black().bold().bright_white(),
                    filter::stats::format_us(rt).on_bright_black().bright_yellow().bold()
                );
                eprintln!("{}", "----------------------------------------------------------".on_bright_black().blue().bold());
                if cfg.verbose { eprintln!("{}", "[MAIN] Finished printing runtime.".dimmed()); }
            } else {
                if cfg.verbose { eprintln!("{}", "[MAIN] Runtime not available.".dimmed()); }
            }
        }
        if main_state.total_dropped_log_messages > 0 {
            eprintln!(
                "{} {}",
                "[WARN] Total log messages dropped due to logger backpressure:".yellow().bold(),
                main_state.total_dropped_log_messages.to_string().on_bright_black().yellow().bold()
            );
            if cfg.verbose { eprintln!("{}", "[MAIN] Finished printing dropped log message count.".dimmed()); }
        } else {
            if cfg.verbose { eprintln!("{}", "[MAIN] No log messages were dropped.".dimmed()); }
        }
    } else {
        if cfg.verbose { eprintln!("{}", "[MAIN] Final statistics flag was already set (expected on signal). Skipping final stats print in main.".dimmed()); }
    }

    eprintln!("{}", "[MAIN] Application exiting successfully.".dimmed());
    Ok(())
}
