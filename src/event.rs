use input_linux_sys::{input_event, EV_KEY}; // timeval is part of input_event
use std::io::{self, Read, Write};
use std::mem::size_of;

/// Reads a single `input_event` from the reader.
/// Returns Ok(None) on EOF.
pub fn read_event(reader: &mut impl Read) -> io::Result<Option<input_event>> {
    let mut buf = vec![0u8; size_of::<input_event>()];
    match reader.read_exact(&mut buf) {
        Ok(()) => {
            // SAFETY: We trust the input source (evdev) to provide valid input_event data.
            // The buffer size matches the struct size exactly.
            let event: input_event = unsafe { std::ptr::read(buf.as_ptr() as *const _) };
            Ok(Some(event))
        }
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None), // Clean EOF
        Err(e) => Err(e),                                              // Other read errors
    }
}

/// Writes a single input_event to the writer.
pub fn write_event(writer: &mut impl Write, event: &input_event) -> io::Result<()> {
    // SAFETY: The input `event` is a valid input_event struct.
    // We create a byte slice representation of the struct to write it out.
    let buf: &[u8] = unsafe {
        std::slice::from_raw_parts(
            event as *const _ as *const u8,
            size_of::<input_event>(),
        )
    };
    writer.write_all(buf)
}

/// Calculates the event timestamp in microseconds.
#[inline]
pub fn event_microseconds(event: &input_event) -> u64 {
    // event.time is timeval { tv_sec: i64, tv_usec: i64 }
    // Convert tv_sec and tv_usec to u64 microseconds
    (event.time.tv_sec as u64) * 1_000_000 + (event.time.tv_usec as u64)
}

/// Checks if the event is a key event.
#[inline]
pub fn is_key_event(event: &input_event) -> bool {
    i32::from(event.type_) == EV_KEY
}
