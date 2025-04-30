use std::io; // Removed Write, keep io for stdin/stderr
use std::process::exit;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

// Signal handling imports
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

// Declare modules
mod cli;
mod event;
mod filter;

use event::{event_microseconds, is_key_event, read_event, write_event};
use filter::BounceFilter;

/// Main entry point for the intercept-bounce filter.
/// Reads input_events from stdin, filters key bounces, and writes results to stdout.
fn main() -> io::Result<()> {
    // Parse command line arguments
    let args = cli::parse_args();

    // Initialize the bounce filter state, potentially guarded by a Mutex
    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new(
        args.window,
        args.verbose,
        args.log_interval,
        args.bypass, // Pass the new bypass flag to the filter
    )));

    // Flag to ensure final stats are printed only once
    let final_stats_printed = Arc::new(AtomicBool::new(false));

    // --- Signal Handling Setup (only if verbose or bypass is active, so stats can be printed) ---
    // We set up signal handling if verbose is on (to print stats) OR if bypass is on (to print bypass status)
    if args.verbose || args.bypass {
        // Clone Arcs for the signal handler thread
        let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?; // <-- Added mut here
        let filter_clone = Arc::clone(&bounce_filter);
        let printed_clone = Arc::clone(&final_stats_printed);

        // Spawn a thread to handle signals asynchronously
        std::thread::spawn(move || {
            for sig in signals.forever() {
                // Attempt to print final stats if not already printed
                if !printed_clone.swap(true, Ordering::SeqCst) {
                    eprintln!("\nReceived signal {}, printing final stats and exiting.", sig);
                    match filter_clone.lock() {
                        Ok(filter) => {
                            // Ignore errors writing stats during signal handling
                            let _ = filter.print_stats(&mut io::stderr());
                        }
                        Err(poisoned) => {
                            // Mutex poisoned - try to recover data if possible, otherwise just log error
                            eprintln!("Error: BounceFilter mutex was poisoned during signal handling!");
                            // Attempt recovery and print stats
                            let _ = poisoned.into_inner().print_stats(&mut io::stderr());
                        }
                    }
                } else {
                    // Avoid redundant message if stats already being printed by main thread exit
                    // eprintln!("Final stats already printed or being printed.");
                }
                // Exit after handling the signal
                exit(128 + sig); // Standard exit code for signals
            }
        });
    }
    // --- End Signal Handling Setup ---


    // Get locked stdin and stdout handles for efficiency
    let mut stdin_locked = io::stdin().lock();
    let mut stdout_locked = io::stdout().lock();

    // Main event processing loop
    while let Some(ev) = read_event(&mut stdin_locked)? {
        // Assume the event should be passed through unless filtered
        let mut pass_through = true;

        // Only apply bounce filtering logic to key events, and only if not in bypass mode
        if is_key_event(&ev) {
            // Lock the filter to check for bounce.
            // The is_bounce method now internally handles bypass, verbose checks, stats, and periodic logging.
            let is_bounce = bounce_filter
                .lock()
                .expect("BounceFilter mutex should not be poisoned in main loop")
                .is_bounce(&ev, event_microseconds(&ev)); // Pass event_us directly

            if is_bounce {
                // It's a bounce (and not in bypass mode), mark it to be dropped.
                // Statistics were updated inside is_bounce if verbose.
                pass_through = false;
            }
            // If it wasn't a bounce (or if in bypass mode), pass_through remains true.
            // The filter state (last_event_us, etc.) was updated inside is_bounce if not in bypass.
        }

        // Write the event to stdout if it wasn't filtered (or if it's not a key event)
        if pass_through {
            write_event(&mut stdout_locked, &ev)?;
        }
        // If !pass_through (i.e., it was a bounce and not in bypass), we simply drop the event here
    } // Closes the while loop (EOF)

    // Print final statistics on clean exit (e.g., EOF) if verbose mode is enabled
    // AND stats haven't been printed by signal handler.
    // Also print if bypass is active, so the bypass status is shown on exit.
    if (args.verbose || args.bypass) && !final_stats_printed.swap(true, Ordering::SeqCst) {
         match bounce_filter.lock() {
             Ok(filter) => {
                 // Ignore potential errors writing stats to stderr at clean exit.
                 let _ = filter.print_stats(&mut io::stderr());
             },
             Err(poisoned) => {
                 // Mutex poisoned - try to recover data if possible, otherwise just log error
                 eprintln!("Error: BounceFilter mutex was poisoned on clean exit!");
                 let _ = poisoned.into_inner().print_stats(&mut io::stderr()); // Attempt recovery
             }
         }
    }

    Ok(())
} // Closes main()
