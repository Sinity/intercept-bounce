use clap::Parser;
use linux_input_sys::input_event;
use std::{
    collections::HashMap,
    io::{self, Read, Write},
    mem::size_of,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Bounce-filter for Interception Tools
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Window (ms) within which repeat edges are discarded
    #[arg(short, long, default_value = "5")]
    window: u664,
}

fn micros_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}

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

       if ev.type_ == libc::EV_KEY {
            let now = micros_now();
            if let Some(&prev) = last.get(&ev.code) {
                if now - prev < window_us {
                    continue; // bounce → drop
                }
            }
            last.insert(ev.code, now);
        }

       stdout.write_all(&buf)?;
    }
    Ok(())
}
