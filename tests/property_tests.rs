//! Property-based tests for the BounceFilter logic using proptest.
//! These tests generate a wide range of input event sequences and debounce times
//! to verify core properties of the filter.

use intercept_bounce::filter::BounceFilter;
use input_linux_sys::{input_event, timeval, EV_KEY, EV_REL, EV_SYN};
use proptest::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

// --- Test Constants ---
const MAX_EVENTS: usize = 1000; // Max number of events in a sequence
const MAX_TIME_DELTA_US: u64 = 1_000_000; // Max time delta between events (1 second)
const MAX_DEBOUNCE_MS: u64 = 500; // Max debounce time to test (500ms)

// --- Strategies ---

// Strategy for generating a key code (0-255 covers most standard keys)
fn arb_keycode() -> impl Strategy<Value = u16> {
    0u16..=255
}

// Strategy for generating a key value (0=release, 1=press, 2=repeat)
fn arb_keyvalue() -> impl Strategy<Value = i32> {
    0i32..=2
}

// Strategy for generating non-key event types (SYN, REL, etc.)
fn arb_nonkey_type() -> impl Strategy<Value = u16> {
    prop_oneof![Just(EV_SYN as u16), Just(EV_REL as u16)]
}


// Strategy for generating a sequence of event data with increasing timestamps.
// Generates a Vec of tuples: (timestamp_us, type, code, value) which IS Debug.
// The input_event structs are constructed inside the test logic.
fn arb_event_sequence_data() -> impl Strategy<Value = Vec<(u64, u16, u16, i32)>> {
    prop::collection::vec(Just(0u64), 0..=MAX_EVENTS).prop_map(|start_times| {
        let mut current_time = 0u64;
        let mut events = Vec::with_capacity(start_times.len());
        // Generate events sequentially ensuring time increases using fastrand
        for _ in 0..start_times.len() {
             let time_delta = fastrand::u64(1..=MAX_TIME_DELTA_US); // Use fastrand directly
             let event_us = current_time.saturating_add(time_delta);
             let event_type = if fastrand::bool() { EV_KEY as u16 } else { if fastrand::bool() { EV_SYN as u16 } else { EV_REL as u16 } };
             let code = fastrand::u16(0..256);
             let value = fastrand::i32(0..3);

             events.push((event_us, event_type, code, value));
             current_time = event_us;
        }
        events
    })
}


// --- Properties ---

proptest! {
    /// Property: A key event that passes the filter (is not a bounce)
    /// should have a time difference less than the `debounce_time`.
    #[test]
    fn prop_debounce_logic(
        event_data in arb_event_sequence_data(),
        debounce_ms in 1u64..=MAX_DEBOUNCE_MS // Debounce time between 1ms and MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();
        let mut last_passed_times: HashMap<(u16, i32), u64> = HashMap::new();

        for (event_us, type_, code, value) in event_data {
            // Construct the event inside the test
            let event = input_event {
                time: timeval {
                    tv_sec: (event_us / 1_000_000) as i64,
                    tv_usec: (event_us % 1_000_000) as i64,
                },
                type_,
                code: if type_ == EV_KEY as u16 { code } else { 0 },
                value: if type_ == EV_KEY as u16 { value } else { 0 },
            };

            let (is_bounce, _diff_us, _last_passed_us_before) = filter.check_event(&event, debounce_time);

            if !is_bounce && crate::event::is_key_event(&event) && event.value != 2 {
                let key = (event.code, event.value);
                if let Some(last_passed) = last_passed_times.get(&key) {
                    // Only assert if event_us >= last_passed (time didn't go backwards)
                    if event_us >= *last_passed { // Check time didn't go backwards before subtraction
                         let diff = event_us - *last_passed; // Safe subtraction
                        // Avoid printing event.time which is not Debug
                        prop_assert!(
                            Duration::from_micros(diff) >= debounce_time,
                            "Passed event type:{} code:{} val:{} at {}us was too close ({}us) to previous passed event at {}us for key {:?}. Debounce time: {:?}",
                            type_, code, value, event_us, diff, last_passed, key, debounce_time
                        );
                    }
                }
                last_passed_times.insert(key, event_us);
            }
        }
    }

    /// Property: All non-key events (EV_SYN, EV_REL, etc.) should always pass the filter.
    #[test]
    fn prop_non_key_events_pass(
        event_data in arb_event_sequence_data(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS // Include 0ms debounce
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();

        for (event_us, type_, code, value) in event_data {
            let event = input_event {
                time: timeval {
                    tv_sec: (event_us / 1_000_000) as i64,
                    tv_usec: (event_us % 1_000_000) as i64,
                },
                type_,
                code: if type_ == EV_KEY as u16 { code } else { 0 },
                value: if type_ == EV_KEY as u16 { value } else { 0 },
            };

            if !crate::event::is_key_event(&event) {
                let (is_bounce, _diff, _last) = filter.check_event(&event, debounce_time);
                // Avoid printing event.time which is not Debug
                prop_assert!(
                    !is_bounce,
                    "Non-key event type:{} code:{} val:{} was incorrectly marked as bounce.",
                    type_, code, value
                );
            }
        }
    }

    /// Property: All key repeat events (value == 2) should always pass the filter.
    #[test]
    fn prop_repeat_events_pass(
        event_data in arb_event_sequence_data(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();

        for (event_us, type_, code, value) in event_data {
             let event = input_event {
                time: timeval {
                    tv_sec: (event_us / 1_000_000) as i64,
                    tv_usec: (event_us % 1_000_000) as i64,
                },
                type_,
                code: if type_ == EV_KEY as u16 { code } else { 0 },
                value: if type_ == EV_KEY as u16 { value } else { 0 },
            };

            if crate::event::is_key_event(&event) && event.value == 2 {
                let (is_bounce, _diff, _last) = filter.check_event(&event, debounce_time);
                // Avoid printing event.time which is not Debug
                prop_assert!(
                    !is_bounce,
                    "Repeat event type:{} code:{} val:{} was incorrectly marked as bounce.",
                    type_, code, value
                );
            }
        }
    }

    /// Property: The relative order of events that pass the filter should be preserved
    /// relative order in the input sequence.
    #[test]
    fn prop_order_preservation(
        event_data in arb_event_sequence_data(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();
        let mut passed_events_ts = Vec::new();

        for (event_us, type_, code, value) in &event_data {
            let event = input_event {
                time: timeval {
                    tv_sec: (*event_us / 1_000_000) as i64,
                    tv_usec: (*event_us % 1_000_000) as i64,
                },
                type_: *type_,
                code: if *type_ == EV_KEY as u16 { *code } else { 0 },
                value: if *type_ == EV_KEY as u16 { *value } else { 0 },
            };

            let (is_bounce, _diff, _last) = filter.check_event(&event, debounce_time);
            if !is_bounce {
                passed_events_ts.push(*event_us); // Push the timestamp directly
            }
        }

        // Check that the timestamps of passed events are non-decreasing
        let mut last_ts = 0u64;
        for &ts in &passed_events_ts {
            prop_assert!(ts >= last_ts, "Passed event timestamps are not non-decreasing: {} followed by {}", last_ts, ts);
            last_ts = ts;
        }
    }
}
