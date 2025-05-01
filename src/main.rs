use std::io::{self, Write}; // Add Write trait here
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

    // --- Bypass Mode ---
    if args.bypass {
        eprintln!("{}", "Bypass mode enabled: Acting as a simple passthrough.".yellow().bold());
        let mut stdin_locked = io::stdin().lock();
        let mut stdout_locked = io::stdout().lock();
        while let Some(ev) = match read_event(&mut stdin_locked) {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!(
                    "{} {}",
                    "Bypass: Error reading input event:".on_bright_black().red().bold(),
                    e
                );
                exit(3);
            }
        } {
            if let Err(e) = write_event(&mut stdout_locked, &ev) {
                // Handle broken pipe gracefully in bypass mode
                if e.kind() == io::ErrorKind::BrokenPipe {
                    eprintln!("{}", "Bypass: Output pipe broken, exiting.".yellow());
                    break; // Exit loop cleanly
                } else {
                    eprintln!(
                        "{} {}",
                        "Bypass: Error writing output event:".on_bright_black().red().bold(),
                        e
                    );
                    exit(4);
                }
            }
        }
        // In bypass mode, we just exit cleanly after the loop finishes (e.g., EOF or broken pipe)
        return Ok(());
    }

    // --- Normal Filtering Mode ---

    // Set high priority for the process (if possible) - only needed for filtering mode
    set_high_priority();

    // Check for the list_devices flag first (already handled if bypass=false)
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
    // list_devices check is now implicitly part of the non-bypass path

    // Proceed with normal filtering mode (bypass is false)
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
                        if stats_json {
                            // Manually construct JSON including runtime for signal exit
                            #[derive(serde::Serialize)]
                            struct SignalJsonOutput<'a> {
                                runtime_us: Option<u64>,
                                meta: filter::stats::Meta, // Re-use Meta struct definition idea
                                stats: &'a filter::stats::StatsCollector,
                            }
                            let runtime = filter.overall_last_event_us.and_then(|last| {
                                filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                            });
                            let meta = filter::stats::Meta { // Assuming Meta is made public or recreated here
                                debounce_time_us: filter.debounce_time_us,
                                log_all_events: filter.log_all_events,
                                log_bounces: filter.log_bounces,
                                log_interval_us: filter.log_interval_us,
                            };
                            let output = SignalJsonOutput { runtime_us: runtime, meta, stats: &filter.stats };
                            let _ = serde_json::to_writer_pretty(io::stderr(), &output);
                            let _ = writeln!(io::stderr());
                        } else {
                            // Normal stderr output on signal
                            let _ = filter.print_stats(&mut io::stderr());
                        }
                    }
                    Err(poisoned) => {
                        eprintln!("{}", "Error: BounceFilter mutex was poisoned during signal handling!".on_bright_black().red().bold());
                        // Attempt to print stats anyway, might be incomplete
                        let filter = poisoned.into_inner();
                         if stats_json {
                            // Best effort JSON on poison
                             #[derive(serde::Serialize)]
                             struct SignalJsonOutput<'a> {
                                 runtime_us: Option<u64>,
                                 meta: filter::stats::Meta,
                                 stats: &'a filter::stats::StatsCollector,
                             }
                             let runtime = filter.overall_last_event_us.and_then(|last| {
                                 filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                             });
                             let meta = filter::stats::Meta {
                                 debounce_time_us: filter.debounce_time_us,
                                 log_all_events: filter.log_all_events,
                                 log_bounces: filter.log_bounces,
                                 log_interval_us: filter.log_interval_us,
                             };
                             let output = SignalJsonOutput { runtime_us: runtime, meta, stats: &filter.stats };
                             let _ = serde_json::to_writer_pretty(io::stderr(), &output);
                             let _ = writeln!(io::stderr());
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
                if stats_json {
                    // Manually construct JSON including runtime for clean exit
                    #[derive(serde::Serialize)]
                    struct CleanJsonOutput<'a> {
                        runtime_us: Option<u64>,
                        meta: filter::stats::Meta,
                        stats: &'a filter::stats::StatsCollector,
                    }
                    let runtime = filter.overall_last_event_us.and_then(|last| {
                        filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                    });
                    let meta = filter::stats::Meta {
                        debounce_time_us: filter.debounce_time_us,
                        log_all_events: filter.log_all_events,
                        log_bounces: filter.log_bounces,
                        log_interval_us: filter.log_interval_us,
                    };
                    let output = CleanJsonOutput { runtime_us: runtime, meta, stats: &filter.stats };
                    let _ = serde_json::to_writer_pretty(io::stderr(), &output);
                    let _ = writeln!(io::stderr());
                } else {
                    // Normal stderr output on clean exit
                    let _ = filter.print_stats(&mut io::stderr());
                }
            }
            Err(poisoned) => {
                eprintln!("{}", "Error: BounceFilter mutex was poisoned on clean exit!".on_bright_black().red().bold());
                // Attempt to print stats anyway
                let filter = poisoned.into_inner();
                 if stats_json {
                     // Best effort JSON on poison
                     #[derive(serde::Serialize)]
                     struct CleanJsonOutput<'a> {
                         runtime_us: Option<u64>,
                         meta: filter::stats::Meta,
                         stats: &'a filter::stats::StatsCollector,
                     }
                     let runtime = filter.overall_last_event_us.and_then(|last| {
                         filter.overall_first_event_us.map(|first| last.saturating_sub(first))
                     });
                     let meta = filter::stats::Meta {
                         debounce_time_us: filter.debounce_time_us,
                         log_all_events: filter.log_all_events,
                         log_bounces: filter.log_bounces,
                         log_interval_us: filter.log_interval_us,
                     };
                     let output = CleanJsonOutput { runtime_us: runtime, meta: meta, stats: &filter.stats };
                     let _ = serde_json::to_writer_pretty(io::stderr(), &output);
                     let _ = writeln!(io::stderr());
                 } else {
                    let _ = filter.print_stats(&mut io::stderr());
                 }
            }
        }
    }

    Ok(())
}
