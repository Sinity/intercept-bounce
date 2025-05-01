use colored::*; // Keep colored for list_input_devices
use input_linux_sys::{input_event, EV_KEY, EV_REL, EV_ABS, EV_MSC, EV_LED, EV_REP, EV_MAX, EV_SYN};
use libc::{self, ioctl, c_ulong}; // Added libc
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind}; // Removed Read, Write
use std::mem::size_of;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd}; // Added RawFd

/// Reads exactly one `input_event` directly from a raw file descriptor using `libc::read`.
///
/// Handles partial reads and EINTR signals by retrying.
/// Returns `Ok(None)` if EOF is reached cleanly *before* starting to read an event.
/// Returns `Err` on I/O errors or if EOF is hit *during* the read of an event.
pub fn read_event_raw(fd: RawFd) -> io::Result<Option<input_event>> {
    let mut buf = vec![0u8; size_of::<input_event>()];
    let mut bytes_read = 0;
    let total_bytes = buf.len();

    // Loop until the entire event structure is read.
    while bytes_read < total_bytes {
        // SAFETY: Calling libc::read is unsafe. We provide a valid pointer
        // derived from a mutable slice, the correct fd, and the remaining length.
        // The file descriptor is assumed to be valid and opened for reading.
        let result = unsafe {
            libc::read(
                fd,
                // Pointer to the next position in the buffer to write to.
                buf.as_mut_ptr().add(bytes_read) as *mut libc::c_void,
                // Number of bytes remaining to fill the buffer.
                total_bytes - bytes_read,
            )
        };

        match result {
            -1 => {
                // Error occurred during read.
                let err = io::Error::last_os_error();
                // If the error is EINTR (Interrupted system call), just retry the read.
                if err.kind() != ErrorKind::Interrupted {
                    return Err(err);
                }
                // If interrupted, the loop continues, retrying the read.
            }
            0 => {
                // EOF (End Of File) reached.
                if bytes_read == 0 {
                    // EOF reached cleanly before reading any part of the current event.
                    return Ok(None);
                } else {
                    // EOF reached unexpectedly after reading *part* of an event.
                    // This indicates a corrupted input stream.
                    return Err(io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "EOF reached mid-event",
                    ));
                }
            }
            n if n > 0 => {
                // Successfully read 'n' bytes.
                bytes_read += n as usize;
            }
            _ => {
                 // Should not happen (e.g. negative result other than -1)
                 return Err(io::Error::new(
                     ErrorKind::Other,
                     "libc::read returned unexpected value",
                 ));
            }
        }
    }

    // If we reach here, we have read total_bytes successfully.
    // Now, convert the byte buffer to an input_event struct.

    // SAFETY: We ensure the buffer has the exact size of input_event.
    // We trust that input_event (from input-linux-sys) has the correct C representation.
    // read_unaligned is used because the alignment of the raw file descriptor
    // stream cannot be guaranteed.
    let ptr = buf.as_ptr();
    // Optional: Check alignment, though read_unaligned handles it.
    // if ptr.align_offset(std::mem::align_of::<input_event>()) != 0 {
    //     return Err(io::Error::new(io::ErrorKind::InvalidData, "input_event alignment error"));
    // }
    let event: input_event = unsafe { std::ptr::read_unaligned(ptr as *const _) };
    Ok(Some(event))
}


/// Writes a single `input_event` directly to a raw file descriptor using `libc::write`.
///
/// Handles partial writes and EINTR signals by retrying.
/// Returns `Err` on I/O errors.
pub fn write_event_raw(fd: RawFd, event: &input_event) -> io::Result<()> {
    let total_bytes = size_of::<input_event>();
    let mut bytes_written = 0;

    // SAFETY: Creates a byte slice representation of the input_event struct.
    // Assumes input_event has a stable C representation.
    let buf: &[u8] = unsafe {
        std::slice::from_raw_parts(
            event as *const _ as *const u8,
            total_bytes,
        )
    };

    // Loop until the entire event structure is written.
    while bytes_written < total_bytes {
        // SAFETY: Calling libc::write is unsafe. We provide a valid pointer
        // derived from the slice, the correct fd, and the remaining length.
        // The file descriptor is assumed to be valid and opened for writing.
        let result = unsafe {
            libc::write(
                fd,
                // Pointer to the start of the remaining data to write.
                buf.as_ptr().add(bytes_written) as *const libc::c_void,
                // Number of bytes remaining to write.
                total_bytes - bytes_written,
            )
        };

        match result {
            -1 => {
                // Error occurred during write.
                let err = io::Error::last_os_error();
                // If the error is EINTR (Interrupted system call), just retry the write.
                if err.kind() != ErrorKind::Interrupted {
                    // Check for BrokenPipe specifically, common when downstream closes.
                    if err.kind() == ErrorKind::BrokenPipe {
                         eprintln!("{}", "[DEBUG] write_event_raw detected BrokenPipe".dimmed());
                    }
                    return Err(err);
                }
                 // If interrupted, the loop continues, retrying the write.
            }
             0 => {
                 // Write returning 0 is unusual for blocking I/O but possible (e.g., fd closed).
                 // Treat it as an error indicating nothing could be written.
                 return Err(io::Error::new(
                     ErrorKind::WriteZero,
                     "libc::write returned 0",
                 ));
             }
            n if n > 0 => {
                // Successfully wrote 'n' bytes.
                bytes_written += n as usize;
            }
             _ => {
                 // Should not happen (e.g. negative result other than -1)
                 return Err(io::Error::new(
                     ErrorKind::Other,
                     "libc::write returned unexpected value",
                 ));
             }
        }
    }
    // If we reach here, all bytes were written successfully.
    Ok(())
}


/// Calculates the event timestamp in microseconds from its timeval struct.
#[inline]
pub fn event_microseconds(event: &input_event) -> u64 {
    // tv_sec and tv_usec are long (i64 on 64-bit systems).
    // Timestamps since epoch are non-negative.
    // Convert to u64 for calculations.
    let sec = event.time.tv_sec as u64;
    let usec = event.time.tv_usec as u64;
    sec * 1_000_000 + usec
}

/// Checks if the event type is EV_KEY.
#[inline]
pub fn is_key_event(event: &input_event) -> bool {
    // Defensive: Only match exactly EV_KEY, not other types
    i32::from(event.type_) == EV_KEY
}


/// Lists available input devices and their capabilities. Requires root privileges.
pub fn list_input_devices() -> io::Result<()> {
    use colored::*;
    eprintln!(
        "{}",
        format!("{:<15} {:<30} {}", "Device", "Name", "Capabilities")
            .on_bright_black()
            .bold()
            .bright_cyan()
    );
    eprintln!("{}", "-------------------------------------------------------------------".on_bright_black().bright_white());

    let mut entries: Vec<_> = fs::read_dir("/dev/input/")?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            if file_name.starts_with("event") {
                // Try to parse the number part to sort numerically
                let num_str = file_name.trim_start_matches("event");
                let num = num_str.parse::<u64>().ok();
                Some((path, num))
            } else {
                None
            }
        })
        .collect();

    // Sort entries numerically by event number
    entries.sort_by_key(|(_, num)| *num);

    for (path, _) in entries {
        let path_str = path.display().to_string();
        let file = match OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&path) {
            Ok(f) => f,
            Err(e) => {
                let msg = format!("{}", e);
                if msg.contains("Permission denied") {
                    eprintln!(
                        "{}",
                        format!("{:<15} {:<30} {}", path_str, "", "Permission Denied")
                            .on_bright_black()
                            .red()
                            .bold()
                    );
                    continue;
                } else {
                    eprintln!(
                        "{}",
                        format!("{:<15} {:<30} Error opening: {}", path_str, "", e)
                            .on_bright_black()
                            .red()
                            .bold()
                    );
                    continue;
                }
            }
        };
        let fd = file.as_raw_fd();

        // Get device name using EVIOCGNAME ioctl
        let mut name_buf = [0u8; 256];
        let device_name = match eviocgname(fd, &mut name_buf) {
            Ok(name) => name,
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Warning: Could not get name for {}: {}", path_str, e)
                        .on_bright_black()
                        .yellow()
                        .bold()
                );
                "<Unknown Name>".to_string()
            }
        };

        // Get supported event types bitmask using EVIOCGBIT ioctl
        let mut capabilities = Vec::new();
        let type_bits_size = (EV_MAX / 8) + 1;
        let mut type_bits_buf: Vec<u8> = vec![0; type_bits_size as usize];

        let mut has_ev_key = false;
        match eviocgbit(fd, 0, &mut type_bits_buf) {
            Ok(_) => {
                if is_bit_set(&type_bits_buf, EV_KEY as usize) {
                    capabilities.push("EV_KEY (Keyboard)");
                    has_ev_key = true;
                }
                if is_bit_set(&type_bits_buf, EV_REL as usize) { capabilities.push("EV_REL (Relative)"); }
                if is_bit_set(&type_bits_buf, EV_ABS as usize) { capabilities.push("EV_ABS (Absolute)"); }
                if is_bit_set(&type_bits_buf, EV_MSC as usize) { capabilities.push("EV_MSC (Misc)"); }
                if is_bit_set(&type_bits_buf, EV_LED as usize) { capabilities.push("EV_LED (LEDs)"); }
                if is_bit_set(&type_bits_buf, EV_REP as usize) { capabilities.push("EV_REP (Repeat)"); }
                if is_bit_set(&type_bits_buf, EV_SYN as usize) { capabilities.push("EV_SYN (Sync)"); }
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("Warning: Could not get capabilities for {}: {}", path_str, e)
                        .on_bright_black()
                        .yellow()
                        .bold()
                );
                capabilities.push("Error getting capabilities");
            }
        }

        if has_ev_key {
            eprintln!(
                "{}",
                format!(
                    "{:<15} {:<30} {}",
                    path_str,
                    device_name,
                    capabilities.join(", ")
                )
                .on_bright_black()
                .bright_white()
            );
        }

        // File closes automatically when dropped
        drop(file);
    }

    eprintln!("{}", "-------------------------------------------------------------------".on_bright_black().bright_white());
    eprintln!(
        "{}",
        "Only devices with 'EV_KEY (Keyboard)' capability are shown above."
            .on_bright_black()
            .bright_cyan()
            .bold()
    );
    eprintln!(
        "{}",
        "You will likely need to run this command with `sudo`."
            .on_bright_black()
            .yellow()
            .bold()
    );

    Ok(())
}

/// Helper function to check if a bit is set in a byte buffer
// Returns true if the bit is set in the buffer, false otherwise.
#[inline] // Add inline hint
fn is_bit_set(buf: &[u8], bit: usize) -> bool {
    let byte_index = bit / 8;
    let bit_index = bit % 8;
    if byte_index < buf.len() {
        (buf[byte_index] & (1 << bit_index)) != 0
    } else {
        false
    }
}

// --- Linux ioctl helpers for EVIOCGNAME and EVIOCGBIT ---

const EVIOCGNAME_LEN: usize = 256;
const EVIOCGNAME_IOCTL: c_ulong = ior(b'E', 0x06, EVIOCGNAME_LEN);
fn eviocgbit_ioctl(ty: u8, len: usize) -> c_ulong {
    ior(b'E', 0x20 + ty, len)
}

// Function to generate ioctl numbers (like _IOR in C)
const fn ior(ty: u8, nr: u8, size: usize) -> c_ulong {
    ((2u64 << 30) | ((size as u64) << 16) | ((ty as u64) << 8) | (nr as u64)) as c_ulong
}

/// Safe wrapper for EVIOCGNAME ioctl
fn eviocgname(fd: RawFd, buf: &mut [u8; 256]) -> io::Result<String> {
    let res = unsafe { ioctl(fd, EVIOCGNAME_IOCTL, buf.as_mut_ptr()) };
    if res < 0 {
        Err(io::Error::last_os_error())
    } else {
        // Find first null byte and convert to string
        let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(String::from_utf8_lossy(&buf[..nul]).to_string())
    }
}

/// Safe wrapper for EVIOCGBIT ioctl
fn eviocgbit(fd: RawFd, ev_type: u8, buf: &mut [u8]) -> io::Result<()> {
    let ioctl_num = eviocgbit_ioctl(ev_type, buf.len());
    let res = unsafe { ioctl(fd, ioctl_num, buf.as_mut_ptr()) };
    if res < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
