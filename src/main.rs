use std::io;
use std::process::exit;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

mod cli;
mod event;
mod filter;

use event::{read_event, write_event};
use filter::BounceFilter;

fn main() -> io::Result<()> {
    let args = cli::parse_args();

    let bounce_filter = Arc::new(Mutex::new(BounceFilter::new(
        args.debounce_time,
        args.log_interval,
        args.log_all_events,
        args.log_bounces,
    )));

    let final_stats_printed = Arc::new(AtomicBool::new(false));

    // Setup signal handling in a separate thread
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGQUIT])?;
    let filter_clone = Arc::clone(&bounce_filter);
    let printed_clone = Arc::clone(&final_stats_printed);

    std::thread::spawn(move || {
        if let Some(sig) = signals.forever().next() {
            if !printed_clone.swap(true, Ordering::SeqCst) {
                eprintln!("\nReceived signal {}, printing final stats and exiting.", sig);
                match filter_clone.lock() {
                    Ok(filter) => {
                        let _ = filter.print_stats(&mut io::stderr()); // Ignore errors writing stats during signal handling
                    }
                    Err(poisoned) => {
                        eprintln!("Error: BounceFilter mutex was poisoned during signal handling!");
                        let _ = poisoned.into_inner().print_stats(&mut io::stderr()); // Attempt recovery
                    }
                }
            }
            exit(128 + sig); // Standard exit code for signals
        }
    });

    let mut stdin_locked = io::stdin().lock();
    let mut stdout_locked = io::stdout().lock();

    // Main event processing loop
    while let Some(ev) = read_event(&mut stdin_locked)? {
        let is_bounce = bounce_filter
            .lock()
            .expect("FATAL: BounceFilter mutex poisoned in main event loop.")
            .process_event(&ev);

        if !is_bounce {
            write_event(&mut stdout_locked, &ev)?;
        }
    } // EOF reached

    // Print final statistics on clean exit
    if !final_stats_printed.swap(true, Ordering::SeqCst) {
         match bounce_filter.lock() {
             Ok(filter) => {
                 let _ = filter.print_stats(&mut io::stderr()); // Ignore errors writing stats at exit
             },
             Err(poisoned) => {
                 eprintln!("Error: BounceFilter mutex was poisoned on clean exit!");
                 let _ = poisoned.into_inner().print_stats(&mut io::stderr()); // Attempt recovery
             }
         }
    }

    Ok(())
}
