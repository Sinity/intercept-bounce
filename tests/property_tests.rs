//! Property-based tests for the BounceFilter logic using proptest.

use input_linux_sys::{EV_KEY, EV_REL, EV_SYN, KEY_MAX};
use intercept_bounce::event;
use intercept_bounce::filter::stats::StatsCollector; // Import StatsCollector
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
            // Generate time delta, ensuring it's at least 1 to avoid zero-time diffs unless explicitly testing that
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
        let mut filter = BounceFilter::new(0);
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
        let mut filter = BounceFilter::new(0);

        for (event_us, _type_, code, value) in event_data { // Prefix unused variable
             let event = key_ev(event_us, code, value); // Use helper from test_helpers

            if !event::is_key_event(&event) {
                let info = filter.check_event(&event, debounce_time);
                prop_assert!(
                    !info.is_bounce,
                    "Non-key event type:{_type_} code:{code} val:{value} at {event_us}us was incorrectly marked as bounce."
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
        let mut filter = BounceFilter::new(0);

        for (event_us, _type_, code, value) in event_data { // Prefix unused variable
             let event = key_ev(event_us, code, value); // Use helper from test_helpers

            if event::is_key_event(&event) && event.value == 2 {
                let info = filter.check_event(&event, debounce_time);
                prop_assert!(
                    !info.is_bounce,
                    "Repeat event type:{_type_} code:{code} val:{value} at {event_us}us was incorrectly marked as bounce."
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
        let mut filter = BounceFilter::new(0);
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

    /// Property: Internal counts in StatsCollector should be consistent.
    /// total_processed == passed_count + dropped_count for each key/value state.
    #[test]
    fn prop_stats_collector_consistency(
        event_data in arb_event_sequence_data(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS,
        near_miss_ms in 0u64..=MAX_DEBOUNCE_MS // Use same range for simplicity
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let near_miss_threshold = Duration::from_millis(near_miss_ms);
        let config = dummy_config_no_arc(debounce_time, near_miss_threshold);

        let mut filter = BounceFilter::new(0);
        let mut stats = StatsCollector::with_capacity();

        // Simulate processing events through the filter and recording in stats
        for (event_us, _type_, code, value) in event_data { // Prefix unused variable
            let event = key_ev(event_us, code, value);
            let info = filter.check_event(&event, debounce_time);

            // Manually calculate last_passed_us for the EventInfo struct
            // This is what the main loop would do before sending to logger
            let _last_passed_us = if event::is_key_event(&event) && event.value != 2 { // Prefix unused variable
                 let key_code_idx = event.code as usize;
                 let key_value_idx = event.value as usize;
                 if key_code_idx < intercept_bounce::filter::FILTER_MAP_SIZE && key_value_idx < intercept_bounce::filter::NUM_KEY_STATES {
                     // Access the filter's internal state (requires filter to be public or have an accessor)
                     // Since filter is not public, we'll rely on the info.last_passed_us provided by check_event
                     info.last_passed_us
                 } else {
                     None
                 }
            } else {
                None // Non-key or repeat events don't have relevant last_passed_us for debounce
            };

            // Create EventInfo struct as it would be sent to the logger
            let event_info_for_stats = EventInfo {
                event,
                event_us,
                is_bounce: info.is_bounce,
                diff_us: info.diff_us, // Diff is only Some if is_bounce is true
                last_passed_us: info.last_passed_us, // This comes from the filter's state
            };

            // Record the event info in the stats collector
            stats.record_event_info_with_config(&event_info_for_stats, &config);
        }

        // After processing all events, check the consistency of the stats
        for key_code_idx in 0..intercept_bounce::filter::FILTER_MAP_SIZE {
            let key_stats = &stats.per_key_stats[key_code_idx];

            // Check consistency for Press state
            prop_assert_eq!(
                key_stats.press.total_processed,
                key_stats.press.passed_count + key_stats.press.dropped_count,
                "Stats inconsistency for key {} (Press)", key_code_idx
            );

            // Check consistency for Release state
            prop_assert_eq!(
                key_stats.release.total_processed,
                key_stats.release.passed_count + key_stats.release.dropped_count,
                "Stats inconsistency for key {} (Release)", key_code_idx
            );

            // Check consistency for Repeat state
            prop_assert_eq!(
                key_stats.repeat.total_processed,
                key_stats.repeat.passed_count + key_stats.repeat.dropped_count,
                "Stats inconsistency for key {} (Repeat)", key_code_idx
            );

            // Check that dropped_count matches the number of timings recorded for bounces
             prop_assert_eq!(
                key_stats.press.dropped_count as usize,
                key_stats.press.timings_us.len(),
                "Bounce timing count mismatch for key {} (Press)", key_code_idx
            );
             prop_assert_eq!(
                key_stats.release.dropped_count as usize,
                key_stats.release.timings_us.len(),
                "Bounce timing count mismatch for key {} (Release)", key_code_idx
            );
             prop_assert_eq!(
                key_stats.repeat.dropped_count as usize,
                key_stats.repeat.timings_us.len(),
                "Bounce timing count mismatch for key {} (Repeat)", key_code_idx
            );

            // Check that near_miss count matches the number of timings recorded for near misses
            let near_miss_press_idx = key_code_idx * intercept_bounce::filter::NUM_KEY_STATES + 1;
            let near_miss_release_idx = key_code_idx * intercept_bounce::filter::NUM_KEY_STATES;
            let near_miss_repeat_idx = key_code_idx * intercept_bounce::filter::NUM_KEY_STATES + 2;

            prop_assert_eq!(
                stats.per_key_near_miss_stats[near_miss_press_idx].timings_us.len(),
                stats.per_key_near_miss_stats[near_miss_press_idx].histogram.count as usize,
                "Near-miss timing count mismatch for key {} (Press)", key_code_idx
            );
             prop_assert_eq!(
                stats.per_key_near_miss_stats[near_miss_release_idx].timings_us.len(),
                stats.per_key_near_miss_stats[near_miss_release_idx].histogram.count as usize,
                "Near-miss timing count mismatch for key {} (Release)", key_code_idx
            );
             prop_assert_eq!(
                stats.per_key_near_miss_stats[near_miss_repeat_idx].timings_us.len(),
                stats.per_key_near_miss_stats[near_miss_repeat_idx].histogram.count as usize,
                "Near-miss timing count mismatch for key {} (Repeat)", key_code_idx
            );
        }

        // Check overall counts consistency
        let total_processed_overall: u64 = stats.per_key_stats.iter().map(|s| s.press.total_processed + s.release.total_processed + s.repeat.total_processed).sum();
        let total_passed_overall: u64 = stats.per_key_stats.iter().map(|s| s.press.passed_count + s.release.passed_count + s.repeat.passed_count).sum();
        let total_dropped_overall: u64 = stats.per_key_stats.iter().map(|s| s.press.dropped_count + s.release.dropped_count + s.repeat.dropped_count).sum();

        prop_assert_eq!(
            stats.key_events_processed,
            total_processed_overall,
            "Overall processed count mismatch"
        );
         prop_assert_eq!(
            stats.key_events_passed,
            total_passed_overall,
            "Overall passed count mismatch"
        );
         prop_assert_eq!(
            stats.key_events_dropped,
            total_dropped_overall,
            "Overall dropped count mismatch"
        );
         prop_assert_eq!(
            stats.key_events_processed,
            stats.key_events_passed + stats.key_events_dropped,
            "Overall processed vs passed+dropped mismatch"
        );

        // Check overall histogram counts match sum of per-key histogram counts
        stats.aggregate_histograms(); // Aggregate before checking overall histograms
        let total_bounce_hist_count: u64 = stats.per_key_stats.iter().map(|s| s.press.bounce_histogram.count + s.release.bounce_histogram.count + s.repeat.bounce_histogram.count).sum();
        let total_near_miss_hist_count: u64 = stats.per_key_near_miss_stats.iter().map(|s| s.histogram.count).sum();

        prop_assert_eq!(
            stats.overall_bounce_histogram.count,
            total_bounce_hist_count,
            "Overall bounce histogram count mismatch"
        );
         prop_assert_eq!(
            stats.overall_near_miss_histogram.count,
            total_near_miss_hist_count,
            "Overall near-miss histogram count mismatch"
        );
    }
}
