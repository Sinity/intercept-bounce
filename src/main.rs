// Main application entry point.
// Orchestrates command-line parsing, thread setup, the main event loop,
// signal handling, and final shutdown/stats reporting.

use colored::*;
use crossbeam_channel::{bounded, Sender, TrySendError}; // For channel communication
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::io::{self, ErrorKind, Write}; // Need ErrorKind, Write
use std::os::unix::io::AsRawFd; // To get raw file descriptors
use std::process::exit;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread; // For spawning logger thread
use std::time::Duration; // For logger config

// Application modules
mod cli;
mod event;
mod filter;
mod logger; // Include the new logger module

// Specific imports
use cli::Args;
use event::{list_input_devices, read_event_raw, write_event_raw}; // Use raw I/O functions
use filter::stats; // Need stats::format_us
use filter::BounceFilter;
use logger::{EventInfo, LogMessage, Logger}; // Use logger types

/// Attempts to set the process priority to the highest level (-20 niceness).
/// Prints a warning if it fails (e.g., due to insufficient permissions).
fn set_high_priority() {
    // Niceness is primarily a Unix concept.
    #[cfg(target_os = "linux")]
    {
        use libc::{setpriority, PRIO_PROCESS};
        unsafe {
            // Use libc to call the setpriority system call.
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
    #[cfg(not(target_os = "linux"))]
    {
        // No-op on non-Linux platforms.
    }
}

/// Holds state specific to the main processing thread, primarily for managing
/// communication with the logger thread and handling log drop warnings.
struct MainState {
    log_sender: Sender<LogMessage>,
    warned_about_dropping: bool, // Have we warned about drops *this session*?
    currently_dropping: bool,    // Are we *currently* dropping messages?
                                 // total_dropped_log_messages: u64, // Optional: Track total drops
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
    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new(
        args.debounce_time,
        args.log_interval,
        args.log_all_events,
        args.log_bounces,
        args.stats_json, // Pass the flag here
    )));

    let final_stats_printed = Arc::new(AtomicBool::new(false));
    let stats_json = args.stats_json;

    // Setup signal handling in a separate thread
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let filter_clone = Arc::clone(&bounce_filter);
    let printed_clone = Arc::clone(&final_stats_printed);

    std::thread::spawn(move || {
        if let Some(sig) = signals.forever().next() {
            if !printed_clone.swap(true, Ordering::SeqCst) {
                eprintln!(
                    "\n{} {}",
                    "Received signal, printing final stats and exiting:".on_bright_black().yellow().bold(),
                    sig
                );
                match filter_clone.lock() {
                    Ok(filter) => {
                        // Calculate runtime
                        let runtime = filter.overall_last_event_us.and_then(|last| {
                            filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                        });

                        if stats_json {
                            // Call the unified JSON printing function from stats.rs
                            filter.stats.print_stats_json(
                                filter.debounce_time_us,
                                filter.log_all_events,
                                filter.log_bounces,
                                filter.log_interval_us,
                                runtime, // Pass runtime
                                io::stderr(),
                            );
                        } else {
                            // Normal stderr output on signal
                            let _ = filter.print_stats(&mut io::stderr());
                        }
                    }
                    Err(poisoned) => {
                        eprintln!("{}", "Error: BounceFilter mutex was poisoned during signal handling!".on_bright_black().red().bold());
                        // Attempt to print stats anyway, might be incomplete
                        let filter = poisoned.into_inner();
                        // Calculate runtime
                        let runtime = filter.overall_last_event_us.and_then(|last| {
                            filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                        });
                         if stats_json {
                            // Best effort JSON on poison - call the unified function
                             filter.stats.print_stats_json(
                                 filter.debounce_time_us,
                                 filter.log_all_events,
                                 filter.log_bounces,
                                 filter.log_interval_us,
                                 runtime, // Pass runtime
                                 io::stderr(),
                             );
                         } else {
                             let _ = filter.print_stats(&mut io::stderr());
                         }
                    }
                }
            }
            exit(128 + sig); // Standard exit code for signals
        }
    });

    let mut stdin_locked = io::stdin().lock();
    let mut stdout_locked = io::stdout().lock();

    // Main event processing loop
    while let Some(ev) = match read_event(&mut stdin_locked) {
        Ok(ev) => ev,
        Err(e) => {
            eprintln!(
                "{} {}",
                "Error reading input event:".on_bright_black().red().bold(),
                e
            );
            exit(3);
        }
    } {
        let is_bounce = match bounce_filter.lock() {
            Ok(mut filter) => filter.process_event(&ev),
            Err(poisoned) => {
                eprintln!("{}", "FATAL: BounceFilter mutex poisoned in main event loop.".on_bright_black().red().bold());
                let mut filter = poisoned.into_inner();
                filter.process_event(&ev)
            }
        };

        if !is_bounce {
            if let Err(e) = write_event(&mut stdout_locked, &ev) {
                eprintln!(
                    "{} {}",
                    "Error writing output event:".on_bright_black().red().bold(),
                    e
                );
                exit(4);
            }
        }
    }

    // Print final statistics on clean exit
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
        match bounce_filter.lock() {
            Ok(filter) => {
                // Calculate runtime
                let runtime = filter.overall_last_event_us.and_then(|last| {
                    filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                });

                if stats_json {
                    // Call the unified JSON printing function from stats.rs
                    filter.stats.print_stats_json(
                        filter.debounce_time_us,
                        filter.log_all_events,
                        filter.log_bounces,
                        filter.log_interval_us,
                        runtime, // Pass runtime
                        io::stderr(),
                    );
                } else {
                    // Normal stderr output on clean exit
                    let _ = filter.print_stats(&mut io::stderr());
                }
            }
            Err(poisoned) => {
                eprintln!("{}", "Error: BounceFilter mutex was poisoned on clean exit!".on_bright_black().red().bold());
                // Attempt to print stats anyway
                let filter = poisoned.into_inner();
                // Calculate runtime
                let runtime = filter.overall_last_event_us.and_then(|last| {
                    filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                });
                 if stats_json {
                     // Best effort JSON on poison - call the unified function
                     filter.stats.print_stats_json(
                         filter.debounce_time_us,
                         filter.log_all_events,
                         filter.log_bounces,
                         filter.log_interval_us,
                         runtime, // Pass runtime
                         io::stderr(),
                     );
                 } else {
                    let _ = filter.print_stats(&mut io::stderr());
                 }
            }
        }
    }

    Ok(())
}
