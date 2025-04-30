use input_linux_sys::{input_event, EV_KEY, EV_REL, EV_ABS, EV_MSC, EV_LED, EV_REP, EV_MAX, EV_SYN};
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::fs;
// Remove unused imports: CStr, AsRawFd

// Import necessary items from nix
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use nix::unistd::close;
// Import the ioctl wrappers and Errno directly
use nix::sys::ioctl;
use nix::errno::Errno;
use nix::Error as NixError; // Keep Nix's error type alias

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


/// Lists available input devices and their capabilities. Requires root privileges.
pub fn list_input_devices() -> io::Result<()> {
    eprintln!("{:<15} {:<30} {}", "Device", "Name", "Capabilities");
    eprintln!("-------------------------------------------------------------------");

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
        // Use nix::fcntl::open to open the device
        let fd = match open(&path, OFlag::O_RDONLY | OFlag::O_NONBLOCK, Mode::empty()) {
            Ok(fd) => fd,
            // Use nix::Error::Sys directly in the pattern
            Err(nix::Error::Sys(errno)) if errno == Errno::EACCES => {
                eprintln!("{:<15} {:<30} Permission Denied", path_str, "");
                continue; // Skip to the next device
            }
            Err(e) => {
                eprintln!("{:<15} {:<30} Error opening: {}", path_str, "", e);
                continue; // Skip to the next device
            }
        };

        // Get device name using nix::sys::ioctl::eviocgname
        // Create a buffer for the name
        let mut name_buf = [0u8; 256]; // Standard buffer size for device names
        let device_name = match ioctl::eviocgname(fd, &mut name_buf) {
            Ok(name_cstr) => name_cstr.to_string_lossy().into_owned(), // Convert CStr to String
            Err(e) => {
                eprintln!("Warning: Could not get name for {}: {}", path_str, e);
                "<Unknown Name>".to_string()
            }
        };

        // Get supported event types bitmask using nix::sys::ioctl::eviocgbit
        // EV_MAX is from input_linux_sys, which should be compatible
        let mut capabilities = Vec::new();
        // Buffer size needed is (EV_MAX / 8) + 1 bytes
        let type_bits_size = (EV_MAX / 8) + 1;
        let mut type_bits_buf: Vec<u8> = vec![0; type_bits_size as usize];

        // Call the ioctl wrapper, passing the mutable buffer
        match ioctl::eviocgbit(fd, 0, &mut type_bits_buf) { // 0 indicates getting EV_ type bits
            Ok(_) => { // eviocgbit returns () on success, buffer is filled
                // Check specific bits using the filled buffer
                if is_bit_set(&type_bits_buf, EV_KEY as usize) { capabilities.push("EV_KEY (Keyboard)"); }
                if is_bit_set(&type_bits_buf, EV_REL as usize) { capabilities.push("EV_REL (Relative)"); }
                if is_bit_set(&type_bits_buf, EV_ABS as usize) { capabilities.push("EV_ABS (Absolute)"); }
                if is_bit_set(&type_bits_buf, EV_MSC as usize) { capabilities.push("EV_MSC (Misc)"); }
                if is_bit_set(&type_bits_buf, EV_LED as usize) { capabilities.push("EV_LED (LEDs)"); }
                if is_bit_set(&type_bits_buf, EV_REP as usize) { capabilities.push("EV_REP (Repeat)"); }
                if is_bit_set(&type_bits_buf, EV_SYN as usize) { capabilities.push("EV_SYN (Sync)"); }
                // Add other types if needed
            }
            Err(e) => {
                eprintln!("Warning: Could not get capabilities for {}: {}", path_str, e);
                capabilities.push("Error getting capabilities");
            }
        }

        eprintln!("{:<15} {:<30} {}",
            path_str,
            device_name,
            capabilities.join(", ")
        );

        // Close the file descriptor using nix::unistd::close
        let _ = close(fd); // Ignore close errors for simplicity here
    }

    eprintln!("-------------------------------------------------------------------");
    eprintln!("Look for devices with 'EV_KEY (Keyboard)' capability.");
    eprintln!("You will likely need to run this command with `sudo`.");

    Ok(())
}

// Helper function to check if a bit is set in a byte buffer
fn is_bit_set(buf: &[u8], bit: usize) -> bool {
    let byte_index = bit / 8;
    let bit_index = bit % 8;
    if byte_index < buf.len() {
        (buf[byte_index] & (1 << bit_index)) != 0
    } else {
        false
    }
}
