use std::io;
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

use event::{read_event, write_event}; // event_microseconds and is_key_event are now used within filter::BounceFilter
use filter::BounceFilter;

/// Main entry point for the intercept-bounce filter.
/// Reads input_event structs from stdin, filters key bounces, and writes results to stdout.
fn main() -> io::Result<()> {
    // Parse command line arguments
    let args = cli::parse_args();

    // Initialize the bounce filter state, potentially guarded by a Mutex
    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new(
        args.window,
        args.verbose,
        args.log_interval,
        args.bypass,
        args.log_events, // Pass the new log_events flag
    )));

    // Flag to ensure final stats are printed only once
    let final_stats_printed = Arc::new(AtomicBool::new(false));

    // --- Signal Handling Setup ---
    // We set up signal handling if verbose is on (to print stats) OR if bypass/log_events is on (to print status)
    if args.verbose || args.bypass || args.log_events {
        // Clone Arcs for the signal handler thread
        let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
        let filter_clone = Arc::clone(&bounce_filter);
        let printed_clone = Arc::clone(&final_stats_printed);

        // Spawn a thread to handle signals asynchronously
        std::thread::spawn(move || {
            // Handle the first signal received. The loop is unnecessary as we exit anyway.
            if let Some(sig) = signals.forever().next() {
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
        // Process the event using the filter.
        // This method handles logging (if enabled), bounce checking (if not bypass),
        // and state/stats updates. It returns true if the event should be dropped.
        let is_bounce = bounce_filter
            .lock()
            .expect("FATAL: BounceFilter mutex poisoned in main event loop.") // More specific message
            .process_event(&ev); // Call the new process_event method

        // Write the event to stdout if it was NOT considered a bounce
        if !is_bounce {
            write_event(&mut stdout_locked, &ev)?;
        }
        // If is_bounce is true, the event is simply dropped (not written to stdout)
    } // Closes the while loop (EOF)

    // Print final statistics on clean exit (e.g., EOF) if verbose mode is enabled
    // OR if bypass/log_events is active (to show status).
    // Ensure stats haven't been printed by signal handler.
    if (args.verbose || args.bypass || args.log_events) && !final_stats_printed.swap(true, Ordering::SeqCst) {
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
