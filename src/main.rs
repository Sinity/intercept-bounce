// Main application entry point.
// Orchestrates command-line parsing, thread setup, the main event loop,
// signal handling, and final shutdown/stats reporting.

use colored::*;
use crossbeam_channel::{bounded, Sender, TrySendError, Receiver}; // Added Receiver
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::io::{self, ErrorKind}; // Removed Write
use std::os::unix::io::AsRawFd; // Removed RawFd
use std::process::exit;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle}; // Added JoinHandle

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
    // Parse command-line arguments using clap.
    let args = cli::parse_args();

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
            Ok(_) => {}
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
                exit(2); // Exit with a specific error code for device listing failure
            }
        }
        return Ok(());
    }

    // Proceed with normal filtering mode
    set_high_priority(); // Attempt to increase priority

    // Shared state setup
    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new()));
    let final_stats_printed = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(true)); // Flag to signal termination

    // --- Channel for Main -> Logger communication ---
    // Use a bounded channel to prevent unbounded memory growth if the logger falls behind.
    // A capacity of 1024 should be sufficient for most bursts.
    let (log_sender, log_receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(1024);

    // --- Logger Thread ---
    let logger_args = args.clone(); // Clone args for the logger thread
    let logger_handle: JoinHandle<StatsCollector> = thread::spawn(move || {
        let mut logger = Logger::new(
            log_receiver,
            logger_args.log_all_events,
            logger_args.log_bounces,
            logger_args.log_interval,
            logger_args.stats_json,
            logger_args.debounce_time * 1000, // Pass debounce time in µs
        );
        logger.run() // Returns final StatsCollector when done
    });

    // --- Signal Handling Thread ---
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let running_clone = Arc::clone(&running);
    let final_stats_printed_clone = Arc::clone(&final_stats_printed);
    let log_sender_clone = log_sender.clone(); // Clone sender for signal handler

    thread::spawn(move || {
        if let Some(sig) = signals.forever().next() {
            eprintln!(
                "\n{} {}",
                "Received signal:".on_bright_black().yellow().bold(),
                sig
            );
            // Signal the main loop and logger thread to stop.
            running_clone.store(false, Ordering::SeqCst);
            // Dropping the sender signals the logger thread to shut down.
            drop(log_sender_clone);
            // Set flag to prevent double printing if main loop also exits cleanly.
            final_stats_printed_clone.store(true, Ordering::SeqCst);
            // Note: Actual stats printing now happens after the main loop exits.
            // We just signal termination here.
        }
    });

    // --- Main Event Loop ---
    let stdin_fd = io::stdin().as_raw_fd();
    let stdout_fd = io::stdout().as_raw_fd();
    let debounce_time_us = args.debounce_time * 1000; // Convert ms to µs once

    // State for managing logger communication backpressure
    let mut main_state = MainState {
        log_sender, // Move the original sender here
        warned_about_dropping: false,
        currently_dropping: false,
        total_dropped_log_messages: 0,
    };

    while running.load(Ordering::SeqCst) {
        match read_event_raw(stdin_fd) {
            Ok(Some(ev)) => {
                let event_us = event_microseconds(&ev);
                let (is_bounce, diff_us, last_passed_us) = {
                    // Lock filter only for the check_event call
                    match bounce_filter.lock() {
                        Ok(mut filter) => filter.check_event(&ev, debounce_time_us),
                        Err(poisoned) => {
                            eprintln!("{}", "FATAL: BounceFilter mutex poisoned in main event loop.".on_bright_black().red().bold());
                            // Attempt to continue with the poisoned guard, might be inconsistent
                            let mut filter = poisoned.into_inner();
                            filter.check_event(&ev, debounce_time_us)
                        }
                    }
                }; // Mutex unlocked here

                // Prepare message for logger thread
                let event_info = EventInfo {
                    event: ev, // Pass the original event struct
                    event_us,
                    is_bounce,
                    diff_us,
                    last_passed_us,
                };

                // Send event info to logger thread (non-blocking)
                match main_state.log_sender.try_send(LogMessage::Event(event_info)) {
                    Ok(_) => {
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
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        // Logger thread likely panicked or exited early
                        eprintln!("{}", "[ERROR] Logger channel disconnected unexpectedly.".red());
                        running.store(false, Ordering::SeqCst); // Stop main loop
                        break; // Exit loop immediately
                    }
                }

                // Write event to stdout if it wasn't a bounce
                if !is_bounce {
                    if let Err(e) = write_event_raw(stdout_fd, &ev) {
                        // Handle broken pipe gracefully (downstream closed)
                        if e.kind() == ErrorKind::BrokenPipe {
                            eprintln!("{}", "[INFO] Output pipe broken, exiting.".dimmed());
                            running.store(false, Ordering::SeqCst); // Signal exit
                            break; // Exit loop
                        } else {
                            eprintln!(
                                "{} {}",
                                "Error writing output event:".on_bright_black().red().bold(),
                                e
                            );
                            running.store(false, Ordering::SeqCst); // Signal exit
                            break; // Exit loop gracefully to allow shutdown
                        }
                    }
                }
            }
            Ok(None) => {
                // Clean EOF on stdin
                running.store(false, Ordering::SeqCst); // Signal exit
                break; // Exit loop
            }
            Err(e) => {
                // Handle read errors
                if e.kind() == ErrorKind::Interrupted {
                    // Interrupted by signal, check running flag
                    if !running.load(Ordering::SeqCst) {
                        eprintln!("{}", "[INFO] Read interrupted by signal, exiting.".dimmed());
                        break; // Exit loop
                    }
                    continue; // Otherwise, retry read
                }
                eprintln!(
                    "\n{} {}",
                    "Error reading input event:".on_bright_black().red().bold(),
                    e
                );
                running.store(false, Ordering::SeqCst); // Signal exit
                break; // Exit loop gracefully to allow shutdown
            }
        }
    }

    // --- Shutdown ---
    eprintln!("{}", "[INFO] Main loop finished, shutting down logger...".dimmed());
    // Drop the sender to signal the logger thread to exit.
    drop(main_state.log_sender);

    // Wait for the logger thread to finish and collect the final stats.
    let final_stats = match logger_handle.join() {
        Ok(stats) => {
            eprintln!("{}", "[INFO] Logger thread joined successfully.".dimmed());
            stats
        }
        Err(e) => {
            eprintln!("{} {:?}", "[ERROR] Logger thread panicked:".red().bold(), e);
            // Return default/empty stats if logger panicked
            StatsCollector::with_capacity()
        }
    };

    // Print final statistics if they haven't been printed by the signal handler already.
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
        eprintln!("{}", "[INFO] Printing final statistics...".dimmed());
        // Get runtime from the BounceFilter
        let runtime_us = {
            match bounce_filter.lock() {
                Ok(filter) => filter.get_runtime_us(),
                Err(_) => {
                    eprintln!("{}", "[WARN] BounceFilter mutex poisoned during final runtime calculation.".yellow());
                    None // Cannot get runtime if poisoned
                }
            }
        };

        // Use the final_stats collected from the logger thread.
        if args.stats_json {
            final_stats.print_stats_json(
                args.debounce_time * 1000, // Pass debounce time in µs
                args.log_all_events,
                args.log_bounces,
                args.log_interval * 1_000_000, // Pass interval in µs
                runtime_us,
                &mut io::stderr().lock(), // Lock stderr for writing
            );
        } else {
            final_stats.print_stats_to_stderr(
                args.debounce_time * 1000, // Pass debounce time in µs
                args.log_all_events,
                args.log_bounces,
                args.log_interval * 1_000_000, // Pass interval in µs
            );
            // Print runtime separately in human-readable mode
            if let Some(rt) = runtime_us {
                 eprintln!(
                     "{} {}",
                     "Total Runtime:".on_bright_black().bold().bright_white(),
                     filter::stats::format_us(rt).on_bright_black().bright_yellow().bold()
                 );
                 eprintln!("{}", "----------------------------------------------------------".on_bright_black().blue().bold());
            }
        }
        // Print total dropped log messages if any occurred
        if main_state.total_dropped_log_messages > 0 {
             eprintln!(
                 "{} {}",
                 "[WARN] Total log messages dropped due to logger backpressure:".yellow().bold(),
                 main_state.total_dropped_log_messages.to_string().on_bright_black().yellow().bold()
             );
        }
    } else {
        eprintln!("{}", "[INFO] Final statistics already printed by signal handler.".dimmed());
    }

    Ok(())
}

// --- Old Buffered I/O Functions (Removed) ---
/*
// Reads exactly one input_event from a buffered reader.
// Returns Ok(None) on EOF.
fn read_event<R: Read>(reader: &mut R) -> io::Result<Option<input_event>> {
    // ... implementation removed ...
}

// Writes a single input_event to a buffered writer.
fn write_event<W: Write>(writer: &mut W, event: &input_event) -> io::Result<()> {
    // ... implementation removed ...
}
*/

// --- Old BounceFilter methods moved/refactored ---
/*
impl BounceFilter {
    // ... old new() ...
    // ... old process_event() ...
    // ... old print_stats() ...
    // ... old log_event() ...
    // ... old log_simple_bounce() ...
}
*/

// --- Old main loop structure (for reference) ---
/*
    // Setup signal handling ...

    let mut stdin_locked = io::stdin().lock();
    let mut stdout_locked = io::stdout().lock();

    // Main event processing loop
    while let Some(ev) = match read_event(&mut stdin_locked) { ... } {
        let is_bounce = match bounce_filter.lock() { ... filter.process_event(&ev) ... };

        if !is_bounce {
            if let Err(e) = write_event(&mut stdout_locked, &ev) { ... }
        }
    }

    // Print final statistics on clean exit ...
*/

// --- Old Signal Handling Logic (for reference) ---
/*
    std::thread::spawn(move || {
        match bounce_filter.lock() {
            Ok(filter) => {
                // Calculate runtime
                let runtime = filter.get_runtime_us(); // Use public method

                // Use the final_stats collected from the logger thread
                if args.stats_json {
                    final_stats.print_stats_json(
                        args.debounce_time * 1000,
                        args.log_all_events,
                        args.log_bounces,
                        args.log_interval * 1_000_000,
                        runtime,
                        &mut io::stderr().lock(),
                    );
                } else {
                    final_stats.print_stats_to_stderr(
                        args.debounce_time * 1000,
                        args.log_all_events,
                        args.log_bounces,
                        args.log_interval * 1_000_000,
                    );
                    // Print runtime separately
                    if let Some(rt) = runtime {
                         eprintln!(
                             "{} {}",
                             "Total Runtime:".on_bright_black().bold().bright_white(),
                             filter::stats::format_us(rt).on_bright_black().bright_yellow().bold()
                         );
                         eprintln!("{}", "----------------------------------------------------------".on_bright_black().blue().bold());
                    }
                }
                // Print total dropped log messages if any occurred
                if main_state.total_dropped_log_messages > 0 {
                     eprintln!(
                         "{} {}",
                         "[WARN] Total log messages dropped due to logger backpressure:".yellow().bold(),
                         main_state.total_dropped_log_messages.to_string().on_bright_black().yellow().bold()
                     );
                }
            }
            Err(poisoned) => {
                eprintln!("{}", "[ERROR] BounceFilter mutex was poisoned on clean exit!".red().bold());
                // Cannot reliably print stats if filter is poisoned
                let filter = poisoned.into_inner();
                // Maybe print the collected final_stats?
                eprintln!("{}", "[INFO] Attempting to print stats collected by logger thread...".dimmed());
                if args.stats_json {
                    final_stats.print_stats_json(
                        args.debounce_time * 1000, args.log_all_events, args.log_bounces,
                        args.log_interval * 1_000_000, None, &mut io::stderr().lock()
                    );
                } else {
                    final_stats.print_stats_to_stderr(
                        args.debounce_time * 1000, args.log_all_events, args.log_bounces,
                        args.log_interval * 1_000_000
                    );
                }
            }
        }
    } else {
        eprintln!("{}", "[INFO] Final statistics already printed by signal handler.".dimmed());
    }
*/
