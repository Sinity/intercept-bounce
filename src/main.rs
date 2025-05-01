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

// Application modules
mod cli;
mod event;
mod filter;
mod logger; // Include the new logger module

// Specific imports
use event::{list_input_devices, read_event_raw, write_event_raw, event_microseconds}; // Use raw I/O functions, added event_microseconds
use filter::BounceFilter;
use logger::{EventInfo, LogMessage, Logger};
use filter::stats::StatsCollector; // Import StatsCollector for final result type

/// Attempts to set the process priority to the highest level (-20 niceness).
/// Prints a warning if it fails (e.g., due to insufficient permissions).
fn set_high_priority() {
    eprintln!("{}", "[MAIN] Attempting to set high process priority...".dimmed());
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
                eprintln!("{}", "[INFO] Process priority set to -20 (highest).".dimmed());
            }
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("{}", "[INFO] set_high_priority is only implemented for Linux.".dimmed());
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

    // Parse command-line arguments using clap.
    let args = cli::parse_args();
    eprintln!("{}", format!("[MAIN] Arguments parsed: {:?}", args).dimmed());

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
                eprintln!("{}", "[MAIN] Device listing complete. Exiting.".dimmed());
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
                eprintln!("{}", "[MAIN] Exiting due to device listing error.".dimmed());
                exit(2); // Exit with a specific error code for device listing failure
            }
        }
        return Ok(());
    }

    // Proceed with normal filtering mode
    set_high_priority(); // Attempt to increase priority

    // Shared state setup
    eprintln!("{}", "[MAIN] Setting up shared state (BounceFilter, AtomicBools)...".dimmed());
    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new()));
    let final_stats_printed = Arc::new(AtomicBool::new(false));
    let main_running = Arc::new(AtomicBool::new(true)); // Flag for main loop
    let logger_running = Arc::new(AtomicBool::new(true)); // Flag for logger loop
    eprintln!("{}", "[MAIN] Shared state initialized.".dimmed());

    // --- Channel for Main -> Logger communication ---
    eprintln!("{}", "[MAIN] Creating bounded channel for logger communication...".dimmed());
    // Use a bounded channel to prevent unbounded memory growth if the logger falls behind.
    // A capacity of 1024 should be sufficient for most bursts.
    // Note: Channel capacity might need tuning based on expected event rates.
    let (log_sender, log_receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(1024);
    eprintln!("{}", "[MAIN] Channel created with capacity 1024.".dimmed());

    // --- Logger Thread ---
    eprintln!("{}", "[MAIN] Spawning logger thread...".dimmed());
    let logger_args = args.clone(); // Clone args for the logger thread
    let logger_running_clone_for_logger = Arc::clone(&logger_running); // Clone for logger thread
    let logger_handle: JoinHandle<StatsCollector> = thread::spawn(move || {
        eprintln!("{}", "[LOGGER] Logger thread started.".dimmed());
        let mut logger = Logger::new(
            log_receiver,
            logger_running_clone_for_logger, // Pass the atomic flag
            logger_args.log_all_events,
            logger_args.log_bounces,
            logger_args.log_interval,
            logger_args.stats_json,
            logger_args.debounce_time * 1000, // Pass debounce time in µs
        );
        eprintln!("{}", "[LOGGER] Logger instance created. Starting run loop.".dimmed());
        let final_stats = logger.run(); // Returns final StatsCollector when done
        eprintln!("{}", "[LOGGER] Logger run loop finished. Returning final stats.".dimmed());
        final_stats
    });
    eprintln!("{}", "[MAIN] Logger thread spawned.".dimmed());


    // --- Signal Handling Thread ---
    eprintln!("{}", "[MAIN] Setting up signal handling thread...".dimmed());
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let main_running_clone = Arc::clone(&main_running);
    let logger_running_clone_for_signal = Arc::clone(&logger_running); // Clone logger flag for signal handler
    let final_stats_printed_clone = Arc::clone(&final_stats_printed);
    let log_sender_clone = log_sender.clone(); // Clone sender for signal handler

    thread::spawn(move || {
        eprintln!("{}", "[SIGNAL] Signal handling thread started.".dimmed());
        if let Some(sig) = signals.forever().next() {
            eprintln!(
                "\n{} {}",
                "[SIGNAL] Received signal:".on_bright_black().yellow().bold(),
                sig
            );
            // Signal the main loop and logger thread to stop.
            eprintln!("{}", "[SIGNAL] Setting main_running flag to false.".dimmed());
            main_running_clone.store(false, Ordering::SeqCst);
            eprintln!("{}", "[SIGNAL] Setting logger_running flag to false.".dimmed());
            logger_running_clone_for_signal.store(false, Ordering::SeqCst); // Signal logger thread directly
            // Dropping the sender signals the logger thread to shut down.
            eprintln!("{}", "[SIGNAL] Dropping log_sender clone to signal logger.".dimmed());
            drop(log_sender_clone);
            // Set flag to prevent double printing if main loop also exits cleanly.
            eprintln!("{}", "[SIGNAL] Setting final_stats_printed flag to true.".dimmed());
            final_stats_printed_clone.store(true, Ordering::SeqCst);
            // Note: Actual stats printing now happens after the main loop exits.
            // We just signal termination here.
            eprintln!("{}", "[SIGNAL] Signal handling complete. Thread exiting.".dimmed());
        }
    });
    eprintln!("{}", "[MAIN] Signal handling thread spawned.".dimmed());


    // --- Main Event Loop ---
    eprintln!("{}", "[MAIN] Entering main event loop.".dimmed());
    let stdin_fd = io::stdin().as_raw_fd();
    let stdout_fd = io::stdout().as_raw_fd();
    let debounce_time_us = args.debounce_time * 1000; // Convert ms to µs once
    eprintln!("{}", format!("[MAIN] Using stdin_fd: {}, stdout_fd: {}, debounce_time_us: {}", stdin_fd, stdout_fd, debounce_time_us).dimmed());


    // State for managing logger communication backpressure
    let mut main_state = MainState {
        log_sender, // Move the original sender here
        warned_about_dropping: false,
        currently_dropping: false,
        total_dropped_log_messages: 0,
    };
    eprintln!("{}", "[MAIN] MainState initialized.".dimmed());


    // How often to check the running flag when read is interrupted.
    let check_interval = Duration::from_millis(100);
    eprintln!("{}", format!("[MAIN] Read check_interval: {:?}", check_interval).dimmed());


    while main_running.load(Ordering::SeqCst) {
        eprintln!("{}", "[MAIN] Loop iteration: Checking main_running flag (true).".dimmed());
        eprintln!("{}", "[MAIN] Attempting to read event from stdin...".dimmed());
        match read_event_raw(stdin_fd) {
            Ok(Some(ev)) => {
                let event_us = event_microseconds(&ev);
                eprintln!("{}", format!("[MAIN] Read event: type={}, code={}, value={}, ts_us={}", ev.type_, ev.code, ev.value, event_us).dimmed());

                eprintln!("{}", "[MAIN] Locking BounceFilter mutex...".dimmed());
                let (is_bounce, diff_us, last_passed_us) = {
                    // Lock filter only for the check_event call
                    match bounce_filter.lock() {
                        Ok(mut filter) => {
                            eprintln!("{}", "[MAIN] BounceFilter mutex locked successfully.".dimmed());
                            let result = filter.check_event(&ev, debounce_time_us);
                            eprintln!("{}", format!("[MAIN] BounceFilter check_event returned: {:?}", result).dimmed()); // Corrected format! and dimmed()
                            result
                        },
                        Err(poisoned) => {
                            eprintln!("{}", "FATAL: BounceFilter mutex poisoned in main event loop.".on_bright_black().red().bold());
                            // Attempt to continue with the poisoned guard, might be inconsistent
                            let mut filter = poisoned.into_inner();
                            let result = filter.check_event(&ev, debounce_time_us);
                            eprintln!("{}", format!("[MAIN] BounceFilter check_event (poisoned) returned: {:?}", result).dimmed()); // Corrected format! and dimmed()
                            result
                        }
                    }
                }; // Mutex unlocked here
                eprintln!("{}", "[MAIN] BounceFilter mutex unlocked.".dimmed());


                // Prepare message for logger thread
                let event_info = EventInfo {
                    event: ev, // Pass the original event struct
                    event_us,
                    is_bounce,
                    diff_us,
                    last_passed_us,
                };
                // Cannot print EventInfo directly due to input_event not implementing Debug
                // eprintln!("{}", format!("[MAIN] Prepared EventInfo for logger: {:?}", event_info).dimmed());
                eprintln!("{}", format!(
                    "[MAIN] Prepared EventInfo for logger: type={}, code={}, value={}, event_us={}, is_bounce={}, diff_us={:?}, last_passed_us={:?}",
                    event_info.event.type_, event_info.event.code, event_info.event.value, event_info.event_us, event_info.is_bounce, event_info.diff_us, event_info.last_passed_us
                ).dimmed());


                // Send event info to logger thread (non-blocking)
                eprintln!("{}", "[MAIN] Attempting to send EventInfo to logger channel...".dimmed());
                match main_state.log_sender.try_send(LogMessage::Event(event_info)) {
                    Ok(_) => {
                        eprintln!("{}", "[MAIN] Successfully sent EventInfo to logger.".dimmed());
                        // If we were dropping, print a recovery message once
                        if main_state.currently_dropping {
                            eprintln!("{}", "[INFO] Logger channel caught up, resuming logging.".dimmed());
                            main_state.currently_dropping = false;
                        }
                    }
                    Err(TrySendError::Full(_)) => {
                        // Channel is full, drop the message
                        main_state.total_dropped_log_messages += 1;
                        if !main_state.warned_about_dropping {
                            eprintln!("{}", "[WARN] Logger channel full, dropping log messages to maintain performance.".yellow());
                            main_state.warned_about_dropping = true; // Warn only once per session
                            main_state.currently_dropping = true;
                        }
                        eprintln!("{}", format!("[MAIN] Logger channel full. Dropped log message. Total dropped: {}", main_state.total_dropped_log_messages).dimmed());
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        // Logger thread likely panicked or exited early
                        eprintln!("{}", "[ERROR] Logger channel disconnected unexpectedly.".red());
                        eprintln!("{}", "[MAIN] Setting main_running flag to false due to logger channel disconnect.".dimmed());
                        main_running.store(false, Ordering::SeqCst); // Stop main loop
                        eprintln!("{}", "[MAIN] Breaking main loop due to logger channel disconnect.".dimmed());
                        break; // Exit loop immediately
                    }
                }

                // Write event to stdout if it wasn't a bounce
                if !is_bounce {
                    eprintln!("{}", "[MAIN] Event passed filter. Attempting to write to stdout...".dimmed());
                    if let Err(e) = write_event_raw(stdout_fd, &ev) {
                        // Handle broken pipe gracefully (downstream closed)
                        if e.kind() == ErrorKind::BrokenPipe {
                            eprintln!("{}", "[INFO] Output pipe broken, exiting.".dimmed());
                            eprintln!("{}", "[MAIN] Setting main_running flag to false due to BrokenPipe.".dimmed());
                            main_running.store(false, Ordering::SeqCst); // Signal exit
                            eprintln!("{}", "[MAIN] Breaking main loop due to BrokenPipe.".dimmed());
                            break; // Exit loop
                        } else {
                            eprintln!(
                                "{} {}",
                                "Error writing output event:".on_bright_black().red().bold(),
                                e
                            );
                            eprintln!("{}", "[MAIN] Setting main_running flag to false due to write error.".dimmed());
                            main_running.store(false, Ordering::SeqCst); // Signal exit
                            eprintln!("{}", "[MAIN] Breaking main loop due to write error.".dimmed());
                            break; // Exit loop gracefully to allow shutdown
                        }
                    } else {
                         eprintln!("{}", "[MAIN] Successfully wrote event to stdout.".dimmed());
                    }
                } else {
                    eprintln!("{}", "[MAIN] Event dropped by filter. Not writing to stdout.".dimmed());
                }
            }
            Ok(None) => {
                // Clean EOF on stdin
                eprintln!("{}", "[MAIN] Received clean EOF on stdin.".dimmed());
                eprintln!("{}", "[MAIN] Setting main_running flag to false due to EOF.".dimmed());
                main_running.store(false, Ordering::SeqCst); // Signal exit
                eprintln!("{}", "[MAIN] Breaking main loop due to EOF.".dimmed());
                break; // Exit loop
            }
            Err(e) => {
                // Handle read errors
                if e.kind() == ErrorKind::Interrupted {
                    eprintln!("{}", "[MAIN] Read interrupted by signal (EINTR).".dimmed());
                    // Interrupted by signal, check running flag
                    // Add a small sleep to avoid tight loop on EINTR if signal handler is slow
                    // or if multiple signals are pending.
                    eprintln!("{}", format!("[MAIN] Sleeping for {:?} before re-checking running flag.", check_interval).dimmed());
                    thread::sleep(check_interval);
                    eprintln!("{}", "[MAIN] Checking main_running flag after EINTR sleep...".dimmed());
                    if !main_running.load(Ordering::SeqCst) {
                        eprintln!("{}", "[MAIN] main_running is false after EINTR. Breaking loop.".dimmed());
                        break; // Exit loop
                    }
                    eprintln!("{}", "[MAIN] main_running is still true after EINTR. Continuing read loop.".dimmed());
                    continue; // Otherwise, retry read
                }
                eprintln!(
                    "\n{} {}",
                    "Error reading input event:".on_bright_black().red().bold(),
                    e
                );
                eprintln!("{}", "[MAIN] Setting main_running flag to false due to read error.".dimmed());
                main_running.store(false, Ordering::SeqCst); // Signal exit
                eprintln!("{}", "[MAIN] Breaking main loop due to read error.".dimmed());
                break; // Exit loop gracefully to allow shutdown
            }
        }
    }

    eprintln!("{}", "[MAIN] Main event loop finished.".dimmed());

    // --- Shutdown ---
    eprintln!("{}", "[MAIN] Starting shutdown process.".dimmed());
    // Drop the sender to signal the logger thread to exit (redundant with atomic flag, but good practice).
    eprintln!("{}", "[MAIN] Dropping main_state.log_sender.".dimmed());
    drop(main_state.log_sender);
    eprintln!("{}", "[MAIN] log_sender dropped.".dimmed());


    // Wait for the logger thread to finish and collect the final stats.
    eprintln!("{}", "[MAIN] Waiting for logger thread to join...".dimmed());
    let final_stats = match logger_handle.join() {
        Ok(stats) => {
            eprintln!("{}", "[MAIN] Logger thread joined successfully.".dimmed());
            stats
        }
        Err(e) => {
            eprintln!("{} {:?}", "[ERROR] Logger thread panicked:".red().bold(), e);
            eprintln!("{}", "[MAIN] Logger thread panicked. Returning default stats.".dimmed());
            // Return default/empty stats if logger panicked
            StatsCollector::with_capacity()
        }
    };
    eprintln!("{}", "[MAIN] Logger thread joined. Final stats collected.".dimmed());


    // Print final statistics if they haven't been printed by the signal handler already.
    // The signal handler now only sets the flag, it doesn't print stats itself.
    eprintln!("{}", "[MAIN] Checking final_stats_printed flag before printing.".dimmed());
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
        eprintln!("{}", "[MAIN] Final stats flag was not set. Proceeding to print final stats.".dimmed());
        // Get runtime from the BounceFilter
        eprintln!("{}", "[MAIN] Locking BounceFilter mutex to get runtime...".dimmed());
        let runtime_us = {
            match bounce_filter.lock() {
                Ok(filter) => {
                    eprintln!("{}", "[MAIN] BounceFilter mutex locked for runtime calculation.".dimmed());
                    let rt = filter.get_runtime_us();
                    eprintln!("{}", format!("[MAIN] BounceFilter runtime_us: {:?}", rt).dimmed());
                    rt
                },
                Err(_) => {
                    eprintln!("{}", "[WARN] BounceFilter mutex poisoned during final runtime calculation.".yellow());
                    eprintln!("{}", "[MAIN] BounceFilter mutex poisoned. Cannot get runtime.".dimmed());
                    None // Cannot get runtime if poisoned
                }
            }
        };
        eprintln!("{}", "[MAIN] BounceFilter mutex unlocked after runtime calculation.".dimmed());


        // Use the final_stats collected from the logger thread.
        if args.stats_json {
            eprintln!("{}", "[MAIN] Printing final stats in JSON format.".dimmed());
            final_stats.print_stats_json(
                args.debounce_time * 1000, // Pass debounce time in µs
                args.log_all_events,
                args.log_bounces,
                args.log_interval * 1_000_000, // Pass interval in µs
                runtime_us,
                &mut io::stderr().lock(), // Lock stderr for writing
            );
            eprintln!("{}", "[MAIN] Finished printing final stats in JSON format.".dimmed());
        } else {
            eprintln!("{}", "[MAIN] Printing final stats in human-readable format.".dimmed());
            final_stats.print_stats_to_stderr(
                args.debounce_time * 1000, // Pass debounce time in µs
                args.log_all_events,
                args.log_bounces,
                args.log_interval * 1_000_000, // Pass interval in µs
            );
            eprintln!("{}", "[MAIN] Finished printing main stats block.".dimmed());
            // Print runtime separately in human-readable mode
            if let Some(rt) = runtime_us {
                 eprintln!(
                     "{} {}",
                     "Total Runtime:".on_bright_black().bold().bright_white(),
                     filter::stats::format_us(rt).on_bright_black().bright_yellow().bold()
                 );
                 eprintln!("{}", "----------------------------------------------------------".on_bright_black().blue().bold());
                 eprintln!("{}", "[MAIN] Finished printing runtime.".dimmed());
            } else {
                 eprintln!("{}", "[MAIN] Runtime not available.".dimmed());
            }
        }
        // Print total dropped log messages if any occurred
        if main_state.total_dropped_log_messages > 0 {
             eprintln!(
                 "{} {}",
                 "[WARN] Total log messages dropped due to logger backpressure:".yellow().bold(),
                 main_state.total_dropped_log_messages.to_string().on_bright_black().yellow().bold()
             );
             eprintln!("{}", "[MAIN] Finished printing dropped log message count.".dimmed());
        } else {
             eprintln!("{}", "[MAIN] No log messages were dropped.".dimmed());
        }
    } else {
        // This path is expected when shutdown is initiated by a signal.
        // The signal handler already set the flag. No need to print anything here.
        eprintln!("{}", "[MAIN] Final statistics flag was already set (expected on signal). Skipping final stats print in main.".dimmed());
    }

    eprintln!("{}", "[MAIN] Application exiting successfully.".dimmed());
    Ok(())
}
