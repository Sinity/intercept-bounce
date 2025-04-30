use std::io::{self, Read, Write};

// Declare modules
mod cli;
mod event;
mod filter;

use event::{event_microseconds, is_key_event, read_event, write_event};
use filter::BounceFilter;

fn main() -> io::Result<()> {
    // Parse command line arguments
    let args = cli::parse_args();

    // Initialize the bounce filter state
    let mut bounce_filter = BounceFilter::new(args.window);

    // Get locked stdin and stdout handles for efficiency
    let mut stdin_locked = io::stdin().lock();
    let mut stdout_locked = io::stdout().lock();

    // Main event processing loop
    loop {
        // Read the next event from stdin
        match read_event(&mut stdin_locked)? {
            Some(ev) => {
                // Assume the event should be passed through unless filtered
                let mut pass_through = true;

                // Only apply bounce filtering to key events
                if is_key_event(&ev) {
                    let event_us = event_microseconds(&ev);
                    // Check if the event is a bounce
                    if bounce_filter.is_bounce(&ev, event_us) {
                        // It's a bounce, mark it to be dropped
                        pass_through = false;
                    }
                    // If it wasn't a bounce, the filter state was updated internally
                }

                // Write the event to stdout if it wasn't filtered
                if pass_through {
                    write_event(&mut stdout_locked, &ev)?;
                }
                // If !pass_through (i.e., it was a bounce), we simply drop the event here
            }
            None => {
                // End of input stream
                break;
            }
        }
    }

    Ok(())
}
