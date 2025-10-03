//! Unit tests for the BounceFilter logic.

use input_linux_sys::input_event;
use intercept_bounce::filter::BounceFilter;
use intercept_bounce::logger::EventInfo;
use std::time::Duration;

// Use the dev-dependency crate for helpers
use test_helpers::*;

// --- Test Helpers ---

// Helper to check a sequence of events against the filter.
// Returns a vector of EventInfo structs.
fn check_sequence(
    filter: &mut BounceFilter,
    events: &[input_event],
    debounce_time: Duration,
) -> Vec<EventInfo> {
    events
        .iter()
        .map(|ev| filter.check_event(ev, debounce_time, false))
        .collect()
}

// --- Basic Bounce Tests ---

#[test]
fn drops_press_bounce() {
    let mut filter = BounceFilter::new(0);
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 / 2, KEY_A, 1); // Bounce
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    // Check e1 (passed)
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].diff_us, None);
    assert_eq!(results[0].last_passed_us, None); // First event
                                                 // Check e2 (bounced)
    assert!(results[1].is_bounce);
    assert_eq!(
        results[1].diff_us,
        Some(DEBOUNCE_TIME.as_micros() as u64 / 2)
    );
    assert_eq!(results[1].last_passed_us, Some(0));
}

#[test]
fn drops_release_bounce() {
    let mut filter = BounceFilter::new(0);
    let e1 = key_ev(0, KEY_A, 0);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 / 2, KEY_A, 0); // Bounce
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    // Check e1 (passed)
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].diff_us, None);
    assert_eq!(results[0].last_passed_us, None);
    // Check e2 (bounced)
    assert!(results[1].is_bounce);
    assert_eq!(
        results[1].diff_us,
        Some(DEBOUNCE_TIME.as_micros() as u64 / 2)
    );
    assert_eq!(results[1].last_passed_us, Some(0));
}

#[test]
fn passes_outside_window() {
    let mut filter = BounceFilter::new(0);
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 + 1, KEY_A, 1); // Outside window
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    // Check e1 (passed)
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].diff_us, None);
    assert_eq!(results[0].last_passed_us, None);
    // Check e2 (passed)
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].diff_us, None);
    assert_eq!(results[1].last_passed_us, Some(0));
}

#[test]
fn passes_at_window_boundary() {
    let mut filter = BounceFilter::new(0);
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64, KEY_A, 1); // Exactly at boundary
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    // Check e1 (passed)
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].diff_us, None);
    assert_eq!(results[0].last_passed_us, None);
    // Check e2 (passed)
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].diff_us, None);
    assert_eq!(results[1].last_passed_us, Some(0)); // Passes (>= check)
}

#[test]
fn drops_just_below_window_boundary() {
    let mut filter = BounceFilter::new(0);
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(DEBOUNCE_TIME.as_micros() as u64 - 1, KEY_A, 1); // Just inside window
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    // Check e1 (passed)
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].diff_us, None);
    assert_eq!(results[0].last_passed_us, None);
    // Check e2 (bounced)
    assert!(results[1].is_bounce);
    assert_eq!(
        results[1].diff_us,
        Some(DEBOUNCE_TIME.as_micros() as u64 - 1)
    ); // Drops (< check)
    assert_eq!(results[1].last_passed_us, Some(0));
}

// --- Independent Filtering Tests ---

#[test]
fn filters_different_keys_independently() {
    let mut filter = BounceFilter::new(0);
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(t / 3, KEY_B, 1); // Pass
    let e3 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e4 = key_ev(t * 2 / 3, KEY_B, 1); // Drop (bounce of e2)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_TIME);
    // e1 (A,1) passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 (B,1) passes
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].last_passed_us, None);
    // e3 (A,1) drops
    assert!(results[2].is_bounce);
    assert_eq!(results[2].diff_us, Some(t / 2));
    assert_eq!(results[2].last_passed_us, Some(0));
    // e4 (B,1) drops
    assert!(results[3].is_bounce);
    assert_eq!(results[3].diff_us, Some(t / 3)); // diff from e2
    assert_eq!(results[3].last_passed_us, Some(t / 3)); // last passed B was e2
}

#[test]
fn filters_press_release_independently() {
    let mut filter = BounceFilter::new(0);
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(t / 4, KEY_A, 0); // Pass (different value)
    let e3 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e4 = key_ev(t * 3 / 4, KEY_A, 0); // Drop (bounce of e2)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_TIME);
    // e1 (A,1) passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 (A,0) passes
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].last_passed_us, None);
    // e3 (A,1) drops
    assert!(results[2].is_bounce);
    assert_eq!(results[2].diff_us, Some(t / 2));
    assert_eq!(results[2].last_passed_us, Some(0));
    // e4 (A,0) drops
    assert!(results[3].is_bounce);
    assert_eq!(results[3].diff_us, Some(t / 2)); // diff from e2
    assert_eq!(results[3].last_passed_us, Some(t / 4)); // last passed A,0 was e2
}

#[test]
fn filters_release_press_independently() {
    let mut filter = BounceFilter::new(0);
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 0); // Pass (first event)
    let e2 = key_ev(t / 2, KEY_A, 1); // Pass (different value)
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    // e1 (A,0) passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 (A,1) passes
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].last_passed_us, None);
}

#[test]
fn independent_filtering_allows_release_after_dropped_press() {
    let mut filter = BounceFilter::new(0);
    let t = DEBOUNCE_TIME.as_micros() as u64;
    // Press A (Pass) -> Press A (Drop) -> Release A (Pass)
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e3 = key_ev(t, KEY_A, 0); // Pass (first release event)
    let results = check_sequence(&mut filter, &[e1, e2, e3], DEBOUNCE_TIME);
    // e1 (A,1) passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 (A,1) drops
    assert!(results[1].is_bounce);
    assert_eq!(results[1].diff_us, Some(t / 2));
    assert_eq!(results[1].last_passed_us, Some(0));
    // e3 (A,0) passes
    assert!(!results[2].is_bounce);
    assert_eq!(results[2].last_passed_us, None); // First A,0 event
}

// --- Special Value/Type Tests ---

#[test]
fn passes_non_key_events() {
    let mut filter = BounceFilter::new(0);
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = non_key_ev(t / 4); // Pass (SYN)
    let e3 = key_ev(t / 2, KEY_A, 1); // Drop (bounce of e1)
    let e4 = non_key_ev(t * 3 / 4); // Pass (SYN)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], DEBOUNCE_TIME);
    // e1 (A,1) passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 (SYN) passes
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].diff_us, None);
    assert_eq!(results[1].last_passed_us, None);
    // e3 (A,1) drops
    assert!(results[2].is_bounce);
    assert_eq!(results[2].diff_us, Some(t / 2));
    assert_eq!(results[2].last_passed_us, Some(0));
    // e4 (SYN) passes
    assert!(!results[3].is_bounce);
    assert_eq!(results[3].diff_us, None);
    assert_eq!(results[3].last_passed_us, None);
}

#[test]
fn passes_key_repeats() {
    let mut filter = BounceFilter::new(0);
    let t = DEBOUNCE_TIME.as_micros() as u64;
    // Key repeats (value 2) are not debounced.
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(500_000, KEY_A, 2); // Pass (Repeat)
    let e3 = key_ev(500_000 + t / 2, KEY_A, 2); // Pass (Repeat)
    let results = check_sequence(&mut filter, &[e1, e2, e3], DEBOUNCE_TIME);
    // e1 (A,1) passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 (A,2) passes (Repeat)
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].diff_us, None);
    assert_eq!(results[1].last_passed_us, None);
    // e3 (A,2) passes (Repeat)
    assert!(!results[2].is_bounce);
    assert_eq!(results[2].diff_us, None);
    assert_eq!(results[2].last_passed_us, None);
}

// --- Edge Case Tests ---

#[test]
fn window_zero_passes_all_key_events() {
    let mut filter = BounceFilter::new(0);
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(1, KEY_A, 1); // Pass (Window 0)
    let e3 = key_ev(2, KEY_A, 0); // Pass
    let e4 = key_ev(3, KEY_A, 0); // Pass (Window 0)
    let results = check_sequence(&mut filter, &[e1, e2, e3, e4], Duration::ZERO);
    // e1 passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 passes
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].last_passed_us, Some(0));
    // e3 passes
    assert!(!results[2].is_bounce);
    assert_eq!(results[2].last_passed_us, None);
    // e4 passes
    assert!(!results[3].is_bounce);
    assert_eq!(results[3].last_passed_us, Some(2));
}

#[test]
fn ignores_configured_keys() {
    let mut filter = BounceFilter::new(0);
    let debounce = DEBOUNCE_TIME;
    let event_press = key_ev(0, KEY_A, 1);
    let event_bounce = key_ev(1, KEY_A, 1);

    let first = filter.check_event(&event_press, debounce, true);
    assert!(!first.is_bounce, "ignored key should pass initial event");

    let second = filter.check_event(&event_bounce, debounce, true);
    assert!(
        !second.is_bounce,
        "ignored key should not be considered a bounce even inside window"
    );
}

#[test]
fn handles_time_going_backwards() {
    let mut filter = BounceFilter::new(0);
    let t = DEBOUNCE_TIME.as_micros() as u64;
    let e1 = key_ev(t * 2, KEY_A, 1); // Pass @ 20ms
    let e2 = key_ev(t, KEY_A, 1); // Pass @ 10ms (time went back)
    let results = check_sequence(&mut filter, &[e1, e2], DEBOUNCE_TIME);
    // e1 passes
    assert!(!results[0].is_bounce);
    assert_eq!(results[0].last_passed_us, None);
    // e2 passes because event_us < last_passed_us results in checked_sub returning None.
    assert!(!results[1].is_bounce);
    assert_eq!(results[1].diff_us, None);
    assert_eq!(results[1].last_passed_us, Some(t * 2));
}

#[test]
fn initial_state_empty() {
    let filter = BounceFilter::new(0);
    // Ensure runtime is None initially.
    assert_eq!(filter.get_runtime_us(), None);
}
