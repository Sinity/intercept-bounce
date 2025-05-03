// tests/property_tests.rs
use intercept_bounce::filter::BounceFilter;
use input_linux_sys::{input_event, timeval, EV_KEY, EV_REL, EV_SYN};
use proptest::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

// --- Constants ---
const MAX_EVENTS: usize = 1000; // Max events per test case
const MAX_TIME_DELTA_US: u64 = 200_000; // Max time jump between events (200ms)
const MAX_DEBOUNCE_MS: u64 = 100; // Max debounce time for tests (100ms)

// --- Proptest Strategies ---

// Strategy for generating key codes (0-255 is common)
fn arb_keycode() -> impl Strategy<Value = u16> {
    0..256u16
}

// Strategy for generating key values (0=release, 1=press, 2=repeat)
fn arb_keyvalue() -> impl Strategy<Value = i32> {
    prop_oneof![Just(0), Just(1), Just(2)]
}

// Strategy for generating non-key event types (e.g., SYN, REL)
fn arb_nonkey_type() -> impl Strategy<Value = u16> {
    prop_oneof![Just(EV_SYN as u16), Just(EV_REL as u16)]
}

// Strategy for generating a single input_event
// Needs access to random generation, so we use prop_map with random::<u64>()
prop_compose! {
    fn arb_input_event_and_time(current_time_us: u64)
                               (time_delta in 1..=MAX_TIME_DELTA_US, event_type in prop_oneof![Just(EV_KEY as u16), arb_nonkey_type()], code in arb_keycode(), value in arb_keyvalue())
                               -> (input_event, u64) {
        let event_us = current_time_us.saturating_add(time_delta);
        let event = input_event {
            time: timeval {
                tv_sec: (event_us / 1_000_000) as i64,
                tv_usec: (event_us % 1_000_000) as i64,
            },
            type_: event_type,
            code: if event_type == EV_KEY as u16 { code } else { 0 }, // Use generated code only for key events
            value: if event_type == EV_KEY as u16 { value } else { 0 }, // Use generated value only for key events
        };
        (event, event_us)
    }
}


// Strategy for generating a sequence of input events with increasing timestamps
fn arb_event_sequence() -> impl Strategy<Value = Vec<input_event>> {
    prop::collection::vec(Just(0u64), 0..=MAX_EVENTS).prop_map(|start_times| {
        let mut current_time = 0u64;
        let mut events = Vec::with_capacity(start_times.len());
        // Generate events sequentially ensuring time increases
        for _ in 0..start_times.len() {
             // Use proptest's implicit generation context for randomness if needed,
             // but arb_input_event_and_time handles it.
             // We need to generate the event based on the *updated* current_time.
             // This requires a slightly different approach than the original flat_map.
             // Let's generate deltas first, then create events.
             let time_delta = (fastrand::u64(1..=MAX_TIME_DELTA_US)); // Use fastrand directly
             let event_us = current_time.saturating_add(time_delta);
             let event_type = if fastrand::bool() { EV_KEY as u16 } else { if fastrand::bool() { EV_SYN as u16 } else { EV_REL as u16 } };
             let code = fastrand::u16(0..256);
             let value = fastrand::i32(0..3);

             let event = input_event {
                 time: timeval {
                     tv_sec: (event_us / 1_000_000) as i64,
                     tv_usec: (event_us % 1_000_000) as i64,
                 },
                 type_: event_type,
                 code: if event_type == EV_KEY as u16 { code } else { 0 },
                 value: if event_type == EV_KEY as u16 { value } else { 0 },
             };
             events.push(event);
             current_time = event_us;
        }
        events
    })
}


// --- Property Tests ---

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))] // Number of test cases

    /// Property: No two *passed* events for the same key code and value (press/release)
    /// should have a time difference less than the `debounce_time`.
    #[test]
    fn prop_debounce_logic(
        events in arb_event_sequence(),
        debounce_ms in 1u64..=MAX_DEBOUNCE_MS // Debounce time between 1ms and MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();
        let mut last_passed_times: HashMap<(u16, i32), u64> = HashMap::new();

        for event in events {
            let event_us = crate::event::event_microseconds(&event);
            let (is_bounce, _diff_us, _last_passed_us_before) = filter.check_event(&event, debounce_time);

            if !is_bounce && crate::event::is_key_event(&event) && event.value != 2 {
                let key = (event.code, event.value);
                if let Some(last_passed) = last_passed_times.get(&key) {
                    // Only assert if event_us >= last_passed (time didn't go backwards)
                    if event_us >= *last_passed {
                        let diff = event_us - *last_passed; // Safe subtraction
                        prop_assert!(
                            Duration::from_micros(diff) >= debounce_time,
                            "Passed event {:?} type:{} code:{} val:{} at {}us was too close ({}us) to previous passed event at {}us for key {:?}. Debounce time: {:?}",
                            event.time, event.type_, event.code, event.value, event_us, diff, last_passed, key, debounce_time
                        );
                    }
                }
                // Always update last passed time for this key/value if it passed
                last_passed_times.insert(key, event_us);
            }
        }
    }

    /// Property: All non-key events (EV_SYN, EV_REL, etc.) should always pass the filter.
    #[test]
    fn prop_non_key_events_pass(
        events in arb_event_sequence(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS // Include 0ms debounce
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();

        for event in events {
            if !crate::event::is_key_event(&event) {
                let (is_bounce, _diff, _last) = filter.check_event(&event, debounce_time);
                prop_assert!(
                    !is_bounce,
                    "Non-key event {:?} type:{} code:{} val:{} was incorrectly marked as bounce.",
                    event.time, event.type_, event.code, event.value
                );
            } else {
                 // Process key events to update filter state correctly
                 filter.check_event(&event, debounce_time);
            }
        }
    }

    /// Property: All key repeat events (value == 2) should always pass the filter.
    #[test]
    fn prop_repeat_events_pass(
        events in arb_event_sequence(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();

        for event in events {
            if crate::event::is_key_event(&event) && event.value == 2 {
                let (is_bounce, _diff, _last) = filter.check_event(&event, debounce_time);
                prop_assert!(
                    !is_bounce,
                    "Repeat event {:?} type:{} code:{} val:{} was incorrectly marked as bounce.",
                     event.time, event.type_, event.code, event.value
                );
            } else {
                // Process other events to update filter state correctly
                filter.check_event(&event, debounce_time);
            }
        }
    }

    /// Property: The relative order of *passed* events should be the same as their
    /// relative order in the input sequence.
    #[test]
    fn prop_order_preservation(
        events in arb_event_sequence(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let mut filter = BounceFilter::new();
        let mut passed_events_ts = Vec::new();

        for event in &events {
            let (is_bounce, _diff, _last) = filter.check_event(event, debounce_time);
            if !is_bounce {
                passed_events_ts.push(crate::event::event_microseconds(event));
            }
        }

        // Check if the timestamps of passed events are monotonically increasing
        let is_sorted = passed_events_ts.windows(2).all(|w| w[0] <= w[1]);
        prop_assert!(
            is_sorted,
            "Order of passed events was not preserved. Passed timestamps: {:?}",
            passed_events_ts
        );
    }
}

// Helper module to access event functions needed by proptests
// This needs to be accessible by the proptest macro expansion
mod event {
    use input_linux_sys::{input_event, EV_KEY};
    #[inline]
    pub fn event_microseconds(event: &input_event) -> u64 {
        let sec = event.time.tv_sec as u64;
        let usec = event.time.tv_usec as u64;
        sec * 1_000_000 + usec
    }
    #[inline]
    pub fn is_key_event(event: &input_event) -> bool {
        i32::from(event.type_) == EV_KEY
    }
}
