use std::io;
use std::process::{exit, Command};
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

    // Set high priority for the process (if possible)
    set_high_priority();

    // Check for the list_devices flag first
    if args.list_devices {
        eprintln!("{}", "Scanning input devices (requires root)...".cyan());
        match list_input_devices() {
            Ok(_) => {},
            Err(e) => {
                eprintln!(
                    "{} {}",
                    "Error listing devices:".red().bold(),
                    e
                );
                eprintln!(
                    "{}",
                    "Note: Listing devices requires read access to /dev/input/event*, typically requiring root privileges."
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
                    "Received signal, printing final stats and exiting:".yellow(),
                    sig
                );
                match filter_clone.lock() {
                    Ok(filter) => {
                        if stats_json {
                            filter.stats.print_stats_json(
                                filter.debounce_time_us,
                                filter.log_all_events,
                                filter.log_bounces,
                                filter.log_interval_us,
                                io::stderr(),
                            );
                        }
                        let _ = filter.print_stats(&mut io::stderr());
                    }
                    Err(poisoned) => {
                        eprintln!("{}", "Error: BounceFilter mutex was poisoned during signal handling!".red());
                        let filter = poisoned.into_inner();
                        if stats_json {
                            filter.stats.print_stats_json(
                                filter.debounce_time_us,
                                filter.log_all_events,
                                filter.log_bounces,
                                filter.log_interval_us,
                                io::stderr(),
                            );
                        }
                        let _ = filter.print_stats(&mut io::stderr());
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
            eprintln!("{} {}", "Error reading input event:".red().bold(), e);
            exit(3);
        }
    } {
        let is_bounce = match bounce_filter.lock() {
            Ok(mut filter) => filter.process_event(&ev),
            Err(poisoned) => {
                eprintln!("{}", "FATAL: BounceFilter mutex poisoned in main event loop.".red().bold());
                let mut filter = poisoned.into_inner();
                filter.process_event(&ev)
            }
        };

        if !is_bounce {
            if let Err(e) = write_event(&mut stdout_locked, &ev) {
                eprintln!("{} {}", "Error writing output event:".red().bold(), e);
                exit(4);
            }
        }
    }

    // Print final statistics on clean exit
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
        match bounce_filter.lock() {
            Ok(filter) => {
                let _ = filter.print_stats(&mut io::stderr());
            },
            Err(poisoned) => {
                eprintln!("{}", "Error: BounceFilter mutex was poisoned on clean exit!".red());
                let _ = poisoned.into_inner().print_stats(&mut io::stderr());
            }
        }
    }

    Ok(())
}
