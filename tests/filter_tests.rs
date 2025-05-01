//! Unit tests for the BounceFilter logic.
//! These tests focus *only* on the state and decision logic within BounceFilter,
//! assuming the logger thread handles stats accumulation separately.

use intercept_bounce::filter::BounceFilter; // Import the filter
use input_linux_sys::{input_event, timeval, EV_KEY, EV_SYN}; // Import event types

// --- Test Constants ---
const KEY_A: u16 = 30; // Example key code
const KEY_B: u16 = 48; // Another key code
const DEBOUNCE_MS: u64 = 10; // Standard debounce time for tests
const DEBOUNCE_US: u64 = DEBOUNCE_MS * 1_000; // Debounce time in microseconds

// --- Test Helpers ---

/// Creates an EV_KEY input_event with a specific microsecond timestamp.
fn key_ev(ts_us: u64, code: u16, value: i32) -> input_event {
    input_event {
        // Corrected: Only one timeval definition
        time: timeval {
            tv_sec: (ts_us / 1_000_000) as i64, // Integer division gives seconds
            tv_usec: (ts_us % 1_000_000) as i64, // Remainder gives microseconds
        },
        type_: EV_KEY as u16, // Event type: Key press/release/repeat
        code,                 // Key code (e.g., KEY_A)
        value,                // Value (1=press, 0=release, 2=repeat)
    }
}

/// Creates a non-key input_event (e.g., EV_SYN) with a specific microsecond timestamp.
fn non_key_ev(ts_us: u64) -> input_event {
    input_event {
        time: timeval {
            tv_sec: (ts_us / 1_000_000) as i64,
            tv_usec: (ts_us % 1_000_000) as i64,
        },
        type_: EV_SYN as u16, // Event type: Synchronization event
        code: 0,              // Typically SYN_REPORT for keyboard events
        value: 0,             // Typically 0 for SYN_REPORT
    }
}

// Helper to check a sequence of events against the filter.
// Returns a vector of tuples: (is_bounce, diff_us, last_passed_us) for each event.
fn check_sequence(
    filter: &mut BounceFilter,
    events: &[input_event],
    debounce_us: u64,
) -> Vec<(bool, Option<u64>, Option<u64>)> {
    events
        .iter()
        .map(|ev| filter.check_event(ev, debounce_us))
        .collect()
}

// --- Basic Bounce Tests ---

#[test]
fn drops_press_bounce() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A again within window (bounce)
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 passes, no previous
    assert_eq!(results[1], (true, Some(DEBOUNCE_US / 2), Some(0))); // e2 bounces, diff=5ms, prev=0
}

#[test]
fn drops_release_bounce() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 0); // Release A at 0ms
    let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 0); // Release A again within window (bounce)
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (true, Some(DEBOUNCE_US / 2), Some(0))); // e2 bounces
}

#[test]
fn passes_outside_window() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(DEBOUNCE_US + 1, KEY_A, 1); // Press A again outside window
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (false, None, Some(0))); // e2 passes, prev=0
}

#[test]
fn passes_at_window_boundary() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(DEBOUNCE_US, KEY_A, 1); // Press A exactly at window boundary
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (false, None, Some(0))); // e2 passes (>= check), prev=0
}

#[test]
fn drops_just_below_window_boundary() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(DEBOUNCE_US - 1, KEY_A, 1); // Press A just inside window
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (true, Some(DEBOUNCE_US - 1), Some(0))); // e2 drops (< check)
}

// --- Independent Filtering Tests ---

#[test]
fn filters_different_keys_independently() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1); // Press A (Pass) @ 0
    let e2 = key_ev(DEBOUNCE_US / 3, KEY_B, 1); // Press B (Pass) @ 3.3ms
    let e3 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) @ 5ms (bounce of e1)
    let e4 = key_ev(DEBOUNCE_US * 2 / 3, KEY_B, 1); // Press B (Drop) @ 6.6ms (bounce of e2)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (B,1) passes
    assert_eq!(results[2], (true, Some(DEBOUNCE_US / 2), Some(0))); // e3 (A,1) drops, prev A,1 was at 0
    assert_eq!(results[3], (true, Some(DEBOUNCE_US / 3), Some(DEBOUNCE_US / 3))); // e4 (B,1) drops, prev B,1 was at 3.3ms
}

#[test]
fn filters_press_release_independently() {
    let mut filter = BounceFilter::new();
    // Scenario: Rapid press/release passes, subsequent bounces drop
    let e1 = key_ev(0, KEY_A, 1); // Press A (Pass) @ 0
    let e2 = key_ev(DEBOUNCE_US / 4, KEY_A, 0); // Release A (Pass) @ 2.5ms - different value
    let e3 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) @ 5ms - bounce of e1
    let e4 = key_ev(DEBOUNCE_US * 3 / 4, KEY_A, 0); // Release A (Drop) @ 7.5ms - bounce of e2
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (A,0) passes
    assert_eq!(results[2], (true, Some(DEBOUNCE_US / 2), Some(0))); // e3 (A,1) drops, prev A,1 was at 0
    assert_eq!(results[3], (true, Some(DEBOUNCE_US / 2), Some(DEBOUNCE_US / 4))); // e4 (A,0) drops, prev A,0 was at 2.5ms
}

#[test]
fn filters_release_press_independently() {
    let mut filter = BounceFilter::new();
    // Scenario: Start with release, then rapid press
    let e1 = key_ev(0, KEY_A, 0); // Release A (Pass) @ 0 - first event
    let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Pass) @ 5ms - different value
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 (A,0) passes
    assert_eq!(results[1], (false, None, None)); // e2 (A,1) passes
}

#[test]
fn independent_filtering_allows_release_after_dropped_press() {
    let mut filter = BounceFilter::new();
    // Press A (Pass) -> Press A (Drop) -> Release A (Pass, because last *passed* release was long ago)
    let e1 = key_ev(0, KEY_A, 1); // Press A (Pass) @ 0
    let e2 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) @ 5ms - bounce of e1
    let e3 = key_ev(DEBOUNCE_US, KEY_A, 0); // Release A (Pass) @ 10ms - first release event seen
    let results = check_sequence(&mut filter, &[e1, e2, e3], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (true, Some(DEBOUNCE_US / 2), Some(0))); // e2 (A,1) drops, prev A,1 was at 0
    assert_eq!(results[2], (false, None, None)); // e3 (A,0) passes, no previous A,0
}

// --- Special Value/Type Tests ---

#[test]
fn passes_non_key_events() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(0, KEY_A, 1); // Press A (Pass) @ 0
    let e2 = non_key_ev(DEBOUNCE_US / 4); // SYN event (Pass) @ 2.5ms
    let e3 = key_ev(DEBOUNCE_US / 2, KEY_A, 1); // Press A (Drop) @ 5ms - bounce of e1
    let e4 = non_key_ev(DEBOUNCE_US * 3 / 4); // SYN event (Pass) @ 7.5ms
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (SYN) passes (non-key)
    assert_eq!(results[2], (true, Some(DEBOUNCE_US / 2), Some(0))); // e3 (A,1) drops, prev A,1 was at 0
    assert_eq!(results[3], (false, None, None)); // e4 (SYN) passes (non-key)
}

#[test]
fn passes_key_repeats() {
    let mut filter = BounceFilter::new();
    // Key repeats (value 2) are NOT debounced
    let e1 = key_ev(0, KEY_A, 1); // Press A (Pass) @ 0
    let e2 = key_ev(500_000, KEY_A, 2); // Repeat A (Pass) @ 500ms
    let e3 = key_ev(500_000 + DEBOUNCE_US / 2, KEY_A, 2); // Repeat A again quickly (Pass) @ 505ms
    let results = check_sequence(&mut filter, &[e1, e2, e3], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 (A,1) passes
    assert_eq!(results[1], (false, None, None)); // e2 (A,2) passes (repeat)
    assert_eq!(results[2], (false, None, None)); // e3 (A,2) passes (repeat)
}

// --- Edge Case Tests ---

#[test]
fn window_zero_passes_all_key_events() {
    let mut filter = BounceFilter::new(); // Debounce time = 0ms
    let e1 = key_ev(0, KEY_A, 1); // Press A (Pass) @ 0
    let e2 = key_ev(1, KEY_A, 1); // Press A again very quickly (Pass) @ 1us
    let e3 = key_ev(2, KEY_A, 0); // Release A (Pass) @ 2us
    let e4 = key_ev(3, KEY_A, 0); // Release A again very quickly (Pass) @ 3us
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], 0); // Pass 0 debounce time
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 passes
    assert_eq!(results[1], (false, None, Some(0))); // e2 passes, prev=0
    assert_eq!(results[2], (false, None, None)); // e3 passes
    assert_eq!(results[3], (false, None, Some(2))); // e4 passes, prev=2
}

#[test]
fn handles_time_going_backwards() {
    let mut filter = BounceFilter::new();
    let e1 = key_ev(DEBOUNCE_US * 2, KEY_A, 1); // Press A at 20ms (Pass)
    let e2 = key_ev(DEBOUNCE_US, KEY_A, 1); // Press A "again" at 10ms (Pass) - time went back
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_US);
    // (is_bounce, diff_us, last_passed_us)
    assert_eq!(results[0], (false, None, None)); // e1 passes
    // e2 passes because event_us < last_passed_us results in checked_sub returning None
    assert_eq!(results[1], (false, None, Some(DEBOUNCE_US * 2)));
}

#[test]
fn initial_state_empty() {
    let filter = BounceFilter::new();
    // Check initial state - BounceFilter is now minimal
    // We can't directly check last_event_us easily, but ensure runtime is None
    assert_eq!(filter.get_runtime_us(), None);
    // We could expose last_event_us for testing if needed, but maybe not necessary
}

// Removed stats_tracking and near_miss_tracking tests as BounceFilter no longer handles stats.
// Those tests should be adapted for StatsCollector in tests/unit_stats.rs.
