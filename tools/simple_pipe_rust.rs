use std::io::{self, Read, Write};
use std::mem::size_of;
use std::process::exit;

// Minimal struct definitions copied from input-linux-sys
// to avoid adding the dependency for this simple tool.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct input_event {
    pub time: timeval,
    pub type_: u16,
    pub code: u16,
    pub value: i32,
}

fn main() -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let event_size = size_of::<input_event>();
    let mut buffer = vec![0u8; event_size];

    loop {
        // Read exactly one event's worth of bytes
        match stdin.read_exact(&mut buffer) {
            Ok(()) => {
                // Successfully read an event, write it directly to stdout
                if let Err(e) = stdout.write_all(&buffer) {
                    if e.kind() == io::ErrorKind::BrokenPipe {
                        // Downstream closed the pipe, exit gracefully
                        eprintln!("Simple_pipe_rust: Output pipe broken, exiting.");
                        break; // Exit loop
                    } else {
                        // Other write error
                        eprintln!("Error writing to stdout: {}", e);
                        exit(1);
                    }
                }
                // Flush stdout to ensure the event is sent immediately
                if let Err(e) = stdout.flush() {
                     if e.kind() == io::ErrorKind::BrokenPipe {
                        eprintln!("Simple_pipe_rust: Output pipe broken on flush, exiting.");
                        break; // Exit loop
                    } else {
                        eprintln!("Error flushing stdout: {}", e);
                        // Continue might be okay here, but exit for safety
                        exit(1);
                    }
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // End of input stream
                break; // Exit loop
            }
            Err(e) => {
                // Other read error
                eprintln!("Error reading from stdin: {}", e);
                exit(1);
            }
        }
    }

    Ok(())
}
