//! Unit tests for the BounceFilter logic.

use input_linux_sys::{input_event, timeval, EV_KEY, EV_SYN};
use intercept_bounce::filter::BounceFilter;
use std::time::Duration;

// --- Test Constants ---
const KEY_A: u16 = 30; // Example key code
const KEY_B: u16 = 48; // Another key code
const DEBOUNCE_TIME: Duration = Duration::from_millis(10); // Standard debounce time for tests

// --- Test Helpers ---

/// Creates an EV_KEY input_event with a specific microsecond timestamp.
fn key_ev(ts_us: u64, code: u16, value: i32) -> input_event {
    input_event {
        time: timeval {
            tv_sec: (ts_us / 1_000_000) as i64,
            tv_usec: (ts_us % 1_000_000) as i64,
        },
        type_: EV_KEY as u16,
        code,
        value,
    }
}

/// Creates a non-key input_event (e.g., EV_SYN) with a specific microsecond timestamp.
fn non_key_ev(ts_us: u64) -> input_event {
    input_event {
        time: timeval {
            tv_sec: (ts_us / 1_000_000) as i64,
            tv_usec: (ts_us % 1_000_000) as i64,
        },
        type_: EV_SYN as u16,
        code: 0,
        value: 0,
    }
}

// Helper to check a sequence of events against the filter.
// Returns a vector of tuples: (is_bounce, diff_us, last_passed_us) for each event.
fn check_sequence(
    filter: &mut BounceFilter,
    events: &[input_event],
    debounce_time: Duration,
) -> Vec<(bool, Option<u64>, Option<u64>)> {
    events
        .iter()
        .map(|ev| filter.check_event(ev, debounce_time))
        .collect()
}

// --- Basic Bounce Tests ---

#[test]
fn drops_press_bounce() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 / 2, KEY_A, 1); // Bounce
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(
        results[1],
        (true, Some(DEBOUNCE_TIME.as_micros() as u64 / 2), Some(0))
    ); // e2 bounces
}

#[test]
fn drops_release_bounce() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 0);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 / 2, KEY_A, 0); // Bounce
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(
        results[1],
        (true, Some(DEBOUNCE_TIME.as_micros() as u64 / 2), Some(0))
    ); // e2 bounces
}

#[test]
fn passes_outside_window() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 + 1, KEY_A, 1); // Outside window
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (false, None, Some(0))); // e2 passes
}

#[test]
fn passes_at_window_boundary() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64, KEY_A, 1); // Exactly at boundary
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (false, None, Some(0))); // e2 passes (>= check)
}

#[test]
fn drops_just_below_window_boundary() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 - 1, KEY_A, 1); // Just inside window
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(
        results[1],
        (true, Some(DEBOUNCE_TIME.as_micros() as u64 - 1), Some(0))
    ); // e2 drops (< check)
}

// --- Independent Filtering Tests ---

#[test]
fn filters_different_keys_independently() {
    let mut filter = BounceFilter::new();
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(t / 3, KEY_B, 1); // Pass
    let e3 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e4 = key_ev(t * 2 / 3, KEY_B, 1); // Drop (bounce of e2)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (B,1) passes
    assert_eq!(results[2], (true, Some(t / 2), Some(0))); // e3 (A,1) drops
    assert_eq!(results[3], (true, Some(t / 3), Some(t / 3))); // e4 (B,1) drops
}

#[test]
fn filters_press_release_independently() {
    let mut filter = BounceFilter::new();
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(t / 4, KEY_A, 0); // Pass (different value)
    let e3 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e4 = key_ev(t * 3 / 4, KEY_A, 0); // Drop (bounce of e2)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (A,0) passes
    assert_eq!(results[2], (true, Some(t / 2), Some(0))); // e3 (A,1) drops
    assert_eq!(results[3], (true, Some(t / 2), Some(t / 4))); // e4 (A,0) drops
}

#[test]
fn filters_release_press_independently() {
    let mut filter = BounceFilter::new();
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 0); // Pass (first event)
    let e2 = key_ev(t / 2, KEY_A, 1); // Pass (different value)
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 (A,0) passes
    assert_eq!(results[1], (false, None, None)); // e2 (A,1) passes
}

#[test]
fn independent_filtering_allows_release_after_dropped_press() {
    let mut filter = BounceFilter::new();
    let t = DEBOUNCE_TIME.as_micros() as u64;
    // Press A (Pass) -> Press A (Drop) -> Release A (Pass)
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e3 = key_ev(t, KEY_A, 0); // Pass (first release event)
    let results = check_sequence(&mut filter, &[e1, e2, e3], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (true, Some(t / 2), Some(0))); // e2 (A,1) drops
    assert_eq!(results[2], (false, None, None)); // e3 (A,0) passes
}

// --- Special Value/Type Tests ---

#[test]
fn passes_non_key_events() {
    let mut filter = BounceFilter::new();
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = non_key_ev(t / 4); // Pass (SYN)
    let e3 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e4 = non_key_ev(t * 3 / 4); // Pass (SYN)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (SYN) passes
    assert_eq!(results[2], (true, Some(t / 2), Some(0))); // e3 (A,1) drops
    assert_eq!(results[3], (false, None, None)); // e4 (SYN) passes
}

#[test]
fn passes_key_repeats() {
    let mut filter = BounceFilter::new();
    let t = DEBOUNCE_TIME.as_micros() as u64;
    // Key repeats (value 2) are not debounced.
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(500_000, KEY_A, 2); // Pass (Repeat)
    let e3 = key_ev(500_000 + t / 2, KEY_A, 2); // Pass (Repeat)
    let results = check_sequence(&mut filter, &[e1, e2, e3], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (A,2) passes
    assert_eq!(results[2], (false, None, None)); // e3 (A,2) passes
}

// --- Edge Case Tests ---

#[test]
fn window_zero_passes_all_key_events() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(1, KEY_A, 1); // Pass (Window 0)
    let e3 = key_ev(2, KEY_A, 0); // Pass
    let e4 = key_ev(3, KEY_A, 0); // Pass (Window 0)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], Duration::ZERO);
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (false, None, Some(0))); // e2 passes
    assert_eq!(results[2], (false, None, None)); // e3 passes
    assert_eq!(results[3], (false, None, Some(2))); // e4 passes
}

#[test]
fn handles_time_going_backwards() {
    let mut filter = BounceFilter::new();
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(t * 2, KEY_A, 1); // Pass @ 20ms
    let e2 = key_ev(t, KEY_A, 1); // Pass @ 10ms (time went back)
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    assert_eq!(results[0], (false, None, None)); // e1 passes
                                                 // e2 passes because event_us < last_passed_us results in checked_sub returning None.
    assert_eq!(results[1], (false, None, Some(t * 2)));
}

#[test]
fn initial_state_empty() {
    let filter = BounceFilter::new();
    // Ensure runtime is None initially.
    assert_eq!(filter.get_runtime_us(), None);
}
