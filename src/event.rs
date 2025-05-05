use input_linux_sys::{
    EV_ABS, EV_KEY, EV_LED, EV_MAX, EV_MSC, EV_REL, EV_REP, EV_SYN, KEY_MAX,
};
// Re-export input_event publicly
pub use input_linux_sys::input_event;

use libc::{self, c_ulong, ioctl};
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind};
use std::mem::size_of;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::{AsRawFd, RawFd};
use tracing::warn;

/// Reads exactly one `input_event` directly from a raw file descriptor using `libc::read`.
///
/// Handles partial reads by retrying internally.
/// Returns `Ok(None)` if EOF is reached cleanly *before* starting to read an event.
/// Returns `Err(ErrorKind::Interrupted)` if the read is interrupted by a signal.
/// Returns `Err` on other I/O errors or if EOF is hit *during* the read of an event.
pub fn read_event_raw(fd: RawFd) -> io::Result<Option<input_event>> {
    let mut buf = vec![0u8; size_of::<input_event>()];
    let mut bytes_read = 0;
    let total_bytes = buf.len();

    while bytes_read < total_bytes {
        let result = unsafe {
            libc::read(
                fd,
                buf.as_mut_ptr().add(bytes_read) as *mut libc::c_void,
                total_bytes - bytes_read,
            )
        };

        match result {
            -1 => {
                let err = io::Error::last_os_error();
                return Err(err);
            }
            0 => {
                if bytes_read == 0 {
                    return Ok(None);
                } else {
                    return Err(io::Error::new(
                        ErrorKind::UnexpectedEof,
                        "EOF reached mid-event",
                    ));
                }
            }
            n if n > 0 => {
                bytes_read += n as usize;
            }
            _ => {
                return Err(io::Error::other("libc::read returned unexpected value"));
            }
        }
    }

    let ptr = buf.as_ptr();
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

    let buf: &[u8] =
        unsafe { std::slice::from_raw_parts(event as *const _ as *const u8, total_bytes) };

    while bytes_written < total_bytes {
        let result = unsafe {
            libc::write(
                fd,
                buf.as_ptr().add(bytes_written) as *const libc::c_void,
                total_bytes - bytes_written,
            )
        };

        match result {
            -1 => {
                let err = io::Error::last_os_error();
                if err.kind() != ErrorKind::Interrupted {
                    return Err(err);
                }
            }
            0 => {
                return Err(io::Error::new(
                    ErrorKind::WriteZero,
                    "libc::write returned 0",
                ));
            }
            n if n > 0 => {
                bytes_written += n as usize;
            }
            _ => {
                return Err(io::Error::other("libc::write returned unexpected value"));
            }
        }
    }
    Ok(())
}

/// Calculates the event timestamp in microseconds from its timeval struct.
/// Returns `u64::MAX` if the calculation overflows.
#[inline]
pub fn event_microseconds(event: &input_event) -> u64 {
    let sec = event.time.tv_sec as u64;
    let usec = event.time.tv_usec as u64; // tv_usec should be non-negative, cast is okay
                                          // Use checked arithmetic to prevent overflow panics from fuzzed/invalid inputs
    sec.checked_mul(1_000_000)
        .and_then(|s| s.checked_add(usec))
        .unwrap_or(u64::MAX)
}

/// Checks if the event type is EV_KEY.
#[inline]
pub fn is_key_event(event: &input_event) -> bool {
    i32::from(event.type_) == EV_KEY
}

/// Lists available input devices and their capabilities. Requires root privileges.
pub fn list_input_devices() -> io::Result<()> {
    eprintln!("{:<15} {:<30} Capabilities", "Device", "Name");
    eprintln!("-------------------------------------------------------------------");

    let mut entries: Vec<_> = fs::read_dir("/dev/input/")?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            if file_name.starts_with("event") {
                let num_str = file_name.trim_start_matches("event");
                let num = num_str.parse::<u64>().ok();
                Some((path, num))
            } else {
                None
            }
        })
        .collect();

    entries.sort_by_key(|(_, num)| *num);

    for (path, _) in entries {
        let path_str = path.display().to_string();
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(f) => f,
            Err(e) => {
                let msg = format!("{e}");
                if msg.contains("Permission denied") { // Keep format! here
                    eprintln!("{:<15} {:<30} Permission Denied", path_str, "");
                    continue;
                } else { // Keep format! here
                    eprintln!("{:<15} {:<30} Error opening: {}", path_str, "", e);
                    continue;
                }
            }
        };
        let fd = file.as_raw_fd();

        let mut name_buf = [0u8; 256];
        let device_name = match eviocgname(fd, &mut name_buf) {
            Ok(name) => name,
            Err(e) => { // Keep %e for Display formatting
                warn!(device=%path_str, error=%e, "Could not get device name via EVIOCGNAME ioctl");
                "<Unknown Name>".to_string()
            }
        };

        let type_bits_size = (EV_MAX / 8) + 1;
        let mut type_bits_buf: Vec<u8> = vec![0; type_bits_size as usize];
        let mut capabilities = Vec::new();

        let mut has_ev_key = false;
        match eviocgbit(fd, 0, &mut type_bits_buf) {
            Ok(_) => {
                if is_bit_set(&type_bits_buf, EV_SYN as usize) {
                    capabilities.push("EV_SYN (Sync)");
                }
                if is_bit_set(&type_bits_buf, EV_KEY as usize) {
                    capabilities.push("EV_KEY (Keyboard)");
                    has_ev_key = true;
                }
                if is_bit_set(&type_bits_buf, EV_REL as usize) {
                    capabilities.push("EV_REL (Relative)");
                }
                if is_bit_set(&type_bits_buf, EV_ABS as usize) {
                    capabilities.push("EV_ABS (Absolute)");
                }
                if is_bit_set(&type_bits_buf, EV_MSC as usize) {
                    capabilities.push("EV_MSC (Misc)");
                }
                if is_bit_set(&type_bits_buf, EV_LED as usize) {
                    capabilities.push("EV_LED (LEDs)");
                }
                if is_bit_set(&type_bits_buf, EV_REP as usize) {
                    capabilities.push("EV_REP (Repeat)");
                }
            }
            Err(e) => { // Keep %e for Display formatting
                warn!(device=%path_str, error=%e, "Could not get device capabilities via EVIOCGBIT ioctl");
                capabilities.push("Error getting capabilities");
            }
        }

        if has_ev_key {
            eprintln!(
                "{:<15} {:<30} {}",
                path_str,
                device_name,
                capabilities.join(", ") // Keep join
            );
        }

        drop(file);
    }

    eprintln!("-------------------------------------------------------------------");
    eprintln!("Only devices with 'EV_KEY (Keyboard)' capability are shown above.");
    eprintln!("You will likely need to run this command with `sudo`.");

    Ok(())
}

/// Helper function to check if a bit is set in a byte buffer
#[inline]
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

const fn ior(ty: u8, nr: u8, size: usize) -> c_ulong {
    ((2u64 << 30) | ((size as u64) << 16) | ((ty as u64) << 8) | (nr as u64)) as c_ulong
}

/// Safe wrapper for EVIOCGNAME ioctl
fn eviocgname(fd: RawFd, buf: &mut [u8; 256]) -> io::Result<String> {
    let res = unsafe { ioctl(fd, EVIOCGNAME_IOCTL, buf.as_mut_ptr()) };
    if res < 0 {
        Err(io::Error::last_os_error())
    } else {
        let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(String::from_utf8_lossy(&buf[..nul]).to_string()) // Keep to_string
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
