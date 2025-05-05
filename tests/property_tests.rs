//! Property-based tests for the BounceFilter logic using proptest.

use input_linux_sys::{EV_KEY, EV_REL, EV_SYN, KEY_MAX};
use intercept_bounce::event;
use intercept_bounce::filter::BounceFilter;
use intercept_bounce::logger::EventInfo;
use proptest::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

// Use the dev-dependency crate for helpers
use test_helpers::*;

// --- Test Constants ---
const MAX_EVENTS: usize = 1000; // Max number of events in a sequence
const MAX_TIME_DELTA_US: u64 = 1_000_000; // Max time delta between events (1 second)
const MAX_DEBOUNCE_MS: u64 = 500; // Max debounce time to test (500ms)

/// Strategy for generating a sequence of event data (timestamp, type, code, value)
/// with increasing timestamps. `input_event` structs are constructed in the tests.
fn arb_event_sequence_data() -> impl Strategy<Value = Vec<(u64, u16, u16, i32)>> {
    prop::collection::vec(Just(0u64), 0..=MAX_EVENTS).prop_map(|start_times| {
        let mut current_time = 0u64;
        let mut events = Vec::with_capacity(start_times.len());
        for _ in 0..start_times.len() {
            let time_delta = fastrand::u64(1..=MAX_TIME_DELTA_US);
            let event_us = current_time.saturating_add(time_delta);
            let event_type = if fastrand::bool() {
                EV_KEY as u16 // Bias towards key events
            } else if fastrand::bool() {
                EV_SYN as u16
            } else {
                EV_REL as u16 // Or relative motion
            };
            let code = fastrand::u16(0..=(KEY_MAX as u16)); // Cast KEY_MAX to u16 for range
            let value = fastrand::i32(0..3); // Random value (press/release/repeat or axis value)

            events.push((event_us, event_type, code, value));
            current_time = event_us;
        }
        events
    })
}

// --- Properties ---

proptest! {
    /// Property: A key event (press/release) that passes the filter should have a time
    /// difference from the *previous passed event of the same key/value* that is
    /// greater than or equal to the `debounce_time`.
    #[test]
    fn prop_debounce_logic(
        event_data in arb_event_sequence_data(),
        debounce_ms in 1u64..=MAX_DEBOUNCE_MS // Test with debounce > 0
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();
        let mut last_passed_times: HashMap<(u16, i32), u64> = HashMap::new();

        for (event_us, type_, code, value) in event_data {
            let event = key_ev(event_us, code, value); // Use helper from test_helpers

            let info: EventInfo = filter.check_event(&event, debounce_time);

            // Check the debounce logic only for non-repeat key events
            if event::is_key_event(&event) && event.value != 2 {
                let key = (event.code, event.value);
                if !info.is_bounce {
                    // If the event passed, check its timing against the last passed event for this key/value
                    if let Some(last_passed) = last_passed_times.get(&key) {
                        // Only assert if time didn't go backwards relative to the last passed event
                        if event_us >= *last_passed {
                            let diff = event_us - *last_passed;
                            prop_assert!(
                                Duration::from_micros(diff) >= debounce_time,
                                "Passed event type:{type_} code:{code} val:{value} at {event_us}us was too close ({diff}us) to previous passed event at {last_passed}us for key {key:?}. Debounce time: {debounce_time:?}"
                            );
                        }
                    }
                    // Record the timestamp of this passed event
                    last_passed_times.insert(key, event_us);
                } else {
                    // If the event bounced, check its timing against the last passed event
                    if let Some(last_passed) = last_passed_times.get(&key) {
                        // Only assert if time didn't go backwards relative to the last passed event
                        if event_us >= *last_passed {
                            let diff = event_us - *last_passed;
                            prop_assert!(
                                Duration::from_micros(diff) < debounce_time,
                                "Bounced event type:{type_} code:{code} val:{value} at {event_us}us was too far ({diff}us) from previous passed event at {last_passed}us for key {key:?}. Debounce time: {debounce_time:?}"
                            );
                        }
                    }
                    // Do not update last_passed_times for bounced events
                }
            }
        }
    }

    /// Property: All non-key events should always pass the filter, regardless of debounce time.
    #[test]
    fn prop_non_key_events_pass(
        event_data in arb_event_sequence_data(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();

        for (event_us, type_, code, value) in event_data {
            let event = key_ev(event_us, code, value); // Use helper from test_helpers

            if !event::is_key_event(&event) {
                let info = filter.check_event(&event, debounce_time);
                prop_assert!(
                    !info.is_bounce,
                    "Non-key event type:{type_} code:{code} val:{value} at {event_us}us was incorrectly marked as bounce."
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
             let event = key_ev(event_us, code, value); // Use helper from test_helpers

            if event::is_key_event(&event) && event.value == 2 {
                let info = filter.check_event(&event, debounce_time);
                prop_assert!(
                    !info.is_bounce,
                    "Repeat event type:{type_} code:{code} val:{value} at {event_us}us was incorrectly marked as bounce."
                );
            }
        }
    }

    /// Property: The relative order of events that pass the filter should be the same
    /// as their relative order in the input sequence.
    #[test]
    fn prop_order_preservation(
        event_data in arb_event_sequence_data(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();
        let mut passed_events_ts = Vec::new();

        for (event_us, _type_, code, value) in &event_data { // Prefix unused variable in pattern
            let event = key_ev(*event_us, *code, *value); // Use helper from test_helpers

            let info = filter.check_event(&event, debounce_time);
            if !info.is_bounce {
                passed_events_ts.push(*event_us);
            }
        }

        // Check that the timestamps of passed events are strictly non-decreasing.
        let mut last_ts = 0u64;
        for &ts in &passed_events_ts {
            prop_assert!(ts >= last_ts, "Passed event timestamps are not non-decreasing: {last_ts} followed by {ts}");
            last_ts = ts;
        }
    }
}
