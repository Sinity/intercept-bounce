use std::io; // Removed unused Write import
use std::process::exit;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use colored::*;

mod cli;
mod event;
mod filter;

use event::{read_event, write_event, list_input_devices};
use filter::BounceFilter;

/// Set process niceness to -20 if possible (warn if not permitted).
fn set_high_priority() {
    #[cfg(target_os = "linux")]
    {
        use libc::{setpriority, PRIO_PROCESS};
        unsafe {
            let res = setpriority(PRIO_PROCESS, 0, -20);
            if res != 0 {
                eprintln!(
                    "{}",
                    "Warning: Unable to set process niceness to -20 (try running as root or with CAP_SYS_NICE)."
                        .yellow()
                );
            } else {
                eprintln!("{}", "Process priority set to -20 (highest)".green());
            }
        }
    }
}

fn main() -> io::Result<()> {
    let args = cli::parse_args();

    // --- Normal Filtering Mode ---

    // Set high priority for the process (if possible)
    set_high_priority();

    // Check for the list_devices flag first
    if args.list_devices {
        eprintln!(
            "{}",
            "Scanning input devices (requires root)..."
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
