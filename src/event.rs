use input_linux_sys::{input_event, EV_KEY};
use std::io::{self, Read, Write};
use std::mem::size_of;

/// Reads a single `input_event` from the reader. Returns Ok(None) on EOF.
pub fn read_event(reader: &mut impl Read) -> io::Result<Option<input_event>> {
    let mut buf = vec![0u8; size_of::<input_event>()];
    match reader.read_exact(&mut buf) {
        Ok(()) => {
            // SAFETY: Assumes the input source provides valid input_event data.
            let event: input_event = unsafe { std::ptr::read(buf.as_ptr() as *const _) };
            Ok(Some(event))
        }
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

/// Writes a single input_event to the writer.
pub fn write_event(writer: &mut impl Write, event: &input_event) -> io::Result<()> {
    // SAFETY: Assumes `event` is a valid input_event. Creates a byte slice representation.
    let buf: &[u8] = unsafe {
        std::slice::from_raw_parts(
            event as *const _ as *const u8,
            size_of::<input_event>(),
        )
    };
    writer.write_all(buf)
}

/// Calculates the event timestamp in microseconds from its timeval.
#[inline]
pub fn event_microseconds(event: &input_event) -> u64 {
    (event.time.tv_sec.max(0) as u64) * 1_000_000 + (event.time.tv_usec.max(0) as u64)
}

/// Checks if the event type is EV_KEY.
#[inline]
pub fn is_key_event(event: &input_event) -> bool {
    i32::from(event.type_) == EV_KEY
}
