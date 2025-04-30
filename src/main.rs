use clap::Parser;
use input_linux_sys::{input_event, EV_KEY}; // Import EV_KEY from input_linux_sys
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    mem::size_of,
    time::/* Duration, */ SystemTime, UNIX_EPOCH, // Removed unused Duration import
};

/// Bounce-filter for Interception Tools
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Window (ms) within which repeat edges are discarded
    #[arg(short, long, default_value = "5")]
    window: u64,
}

// This function is no longer needed as we will use the event timestamp
// fn micros_now() -> u64 {
//     SystemTime::now()
//         .duration_since(UNIX_EPOCH)
//         .unwrap()
//         .as_micros() as u64
// }

fn main() -> io::Result<()> {
    let args = Args::parse();
    let window_us = args.window * 1_000; // ms → µs
    let mut last: HashMap<u16, u64> = HashMap::new();

    let mut stdin  = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut buf    = vec![0u8; size_of::<input_event>()];

    loop {
        if stdin.read_exact(&mut buf).is_err() {
            break; // EOF
        }

        // SAFETY: evdev always gives us exactly sizeof(input_event) bytes
        let ev: input_event = unsafe { std::ptr::read(buf.as_ptr() as *const _) };

        // Only process key events (type 1)
        if i32::from(ev.type_) != EV_KEY {
            stdout.write_all(&buf)?; // Pass non-key events through
            continue;
        }

        // Calculate event timestamp in microseconds from event.time
        // event.time is timeval { tv_sec: i64, tv_usec: i64 }
        // Convert tv_sec and tv_usec to u64 microseconds
        let event_us = (ev.time.tv_sec as u64) * 1_000_000 + (ev.time.tv_usec as u64);

        let last_us = last.get(&ev.code);

        let is_bounce = match last_us {
            Some(&last_us) => {
                // Check if the time difference is within the window
                // Use event_us for comparison. Use checked_sub to handle potential time jumps backwards.
                event_us.checked_sub(last_us)
                    .map_or(false, |diff| diff < window_us)
            }
            None => false, // First event for this code is never a bounce
        };

        if !is_bounce {
            // Not a bounce, write the event and update the last timestamp
            stdout.write_all(&buf)?;
            // Update the last timestamp with the event's timestamp
            last.insert(ev.code, event_us);
        }
        // If it is a bounce, do nothing (drop the event)
    }
    Ok(())
}
