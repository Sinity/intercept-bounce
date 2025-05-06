// fuzz/fuzz_targets/fuzz_core_filter.rs
#![no_main]

use input_linux_sys::input_event;
use intercept_bounce::filter::BounceFilter;
use libfuzzer_sys::fuzz_target;
use std::mem::size_of;
use std::time::Duration;

// Define a reasonable max number of events to process from fuzz data
// to prevent excessively long fuzz runs.
const MAX_EVENTS_PER_FUZZ_CASE: usize = 1000;

fuzz_target!(|data: &[u8]| {
    // Treat the input data as a stream of input_event structs.
    let event_size = size_of::<input_event>();
    if data.len() < event_size {
        return; // Not enough data for even one event
    }

    let num_events = std::cmp::min(data.len() / event_size, MAX_EVENTS_PER_FUZZ_CASE);
    // Explicitly create BounceFilter with ring_buffer_size 0 for fuzzing
    let mut filter = BounceFilter::new(0);
    // Use a fixed, non-zero debounce time for fuzzing consistency
    let debounce_time = Duration::from_millis(10);

    for i in 0..num_events {
        let offset = i * event_size;
        // Ensure we don't read past the end of the data slice
        if offset + event_size > data.len() {
            break;
        }
        let event_bytes = &data[offset..offset + event_size];

        // Safely read the event struct from the byte slice.
        // Using read_unaligned because fuzz data might not be aligned.
        // Ensure the pointer is valid before reading.
        if event_bytes.len() == event_size {
            let event: input_event =
                unsafe { std::ptr::read_unaligned(event_bytes.as_ptr() as *const _) };

            // Call the function under test. The primary goal of fuzzing here
            // is to find panics, crashes, hangs, or memory issues within check_event
            // when processing potentially malformed or unexpected event data.
            // The function now returns an EventInfo struct.
            let _event_info = filter.check_event(&event, debounce_time);

            // Optional: Add basic assertions if specific invariants should hold even with garbage input.
            // For example, ensure runtime calculation doesn't panic.
            let _ = filter.get_runtime_us();
        } else {
            // This case should ideally not be reached due to the outer check,
            // but handle defensively.
            break;
        }
    }
});
