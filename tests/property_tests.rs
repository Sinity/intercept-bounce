//! Property-based tests for the BounceFilter logic using proptest.

use input_linux_sys::{input_event, timeval, EV_KEY, EV_REL, EV_SYN, KEY_MAX};
use intercept_bounce::event;
use intercept_bounce::filter::stats::{
    StatsCollector, MAX_BOUNCE_TIMING_SAMPLES, MAX_NEAR_MISS_TIMING_SAMPLES,
};
use intercept_bounce::filter::{BounceFilter, FILTER_MAP_SIZE, NUM_KEY_STATES};
use intercept_bounce::logger::EventInfo;
use proptest::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

// Use the dev-dependency crate for helpers
use test_helpers::*;

// --- Test Constants ---
const MAX_EVENTS: usize = 200; // Max number of events in a sequence
const MAX_TIME_DELTA_US: u64 = 1_000_000; // Max time delta between events (1 second)
const MAX_DEBOUNCE_MS: u64 = 500; // Max debounce time to test (500ms)

/// Strategy for generating a sequence of event data (timestamp, type, code, value)
/// with non-decreasing timestamps. `input_event` structs are constructed in the tests.
type EventDatum = (u64, u16, u16, i32);

fn arb_event_sequence_data() -> impl Strategy<Value = Vec<EventDatum>> {
    prop::collection::vec(
        (
            0u64..=MAX_TIME_DELTA_US,
            prop_oneof![
                Just(EV_KEY as u16),
                Just(EV_SYN as u16),
                Just(EV_REL as u16)
            ],
            0u16..=KEY_MAX as u16,
            -2i32..=2,
        ),
        0..=MAX_EVENTS,
    )
    .prop_map(|items| {
        let mut current_time = 0u64;
        items
            .into_iter()
            .map(|(delta, ty, code, value)| {
                current_time = current_time.saturating_add(delta);
                let adjusted_value = if ty == EV_KEY as u16 {
                    match value.rem_euclid(3) {
                        0 => 0,
                        1 => 1,
                        _ => 2,
                    }
                } else {
                    value
                };
                (current_time, ty, code, adjusted_value)
            })
            .collect()
    })
}

fn build_event(event_us: u64, event_type: u16, code: u16, value: i32) -> input_event {
    if event_type == EV_KEY as u16 {
        key_ev(event_us, code, value)
    } else {
        input_event {
            time: timeval {
                tv_sec: (event_us / 1_000_000) as i64,
                tv_usec: (event_us % 1_000_000) as i64,
            },
            type_: event_type,
            code,
            value,
        }
    }
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

        for (event_us, event_type, code, value) in event_data {
            let event = build_event(event_us, event_type, code, value);
            let info: EventInfo = filter.check_event(&event, debounce_time, false);

            // Check the debounce logic only for non-repeat key events
            if event::is_key_event(&event) && event.value != 2 {
                let key = (event.code, event.value);
                if !info.is_bounce {
                    if let Some(&last_passed) = last_passed_times.get(&key) {
                        if info.event_us >= last_passed {
                            let diff = info.event_us - last_passed;
                            prop_assert!(
                                Duration::from_micros(diff) >= debounce_time,
                                "Passed event type:{event_type} code:{code} val:{value} at {event_us}us was too close ({diff}us) to previous passed event at {last_passed}us for key {key:?}. Debounce time: {debounce_time:?}",
                                event_us = info.event_us,
                                diff = diff,
                                last_passed = last_passed,
                                debounce_time = debounce_time
                            );
                        }
                    }
                    last_passed_times.insert(key, info.event_us);
                } else if let Some(&last_passed) = last_passed_times.get(&key) {
                    if info.event_us >= last_passed {
                        let diff = info.event_us - last_passed;
                        if let Some(debounce_diff) = info.diff_us {
                            prop_assert!(
                                Duration::from_micros(debounce_diff) < debounce_time,
                                "Bounced event type:{event_type} code:{code} val:{value} at {event_us}us reported diff {debounce_diff}us which is not within debounce window {debounce_time:?}",
                                event_us = info.event_us,
                                debounce_diff = debounce_diff,
                                debounce_time = debounce_time
                            );
                        }
                        prop_assert!(
                            Duration::from_micros(diff) < debounce_time,
                            "Bounced event type:{event_type} code:{code} val:{value} at {event_us}us was too far ({diff}us) from previous passed event at {last_passed}us for key {key:?}. Debounce time: {debounce_time:?}",
                            event_us = info.event_us,
                            diff = diff,
                            last_passed = last_passed,
                            debounce_time = debounce_time
                        );
                    }
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

        for (event_us, event_type, code, value) in event_data {
            let event = build_event(event_us, event_type, code, value);

            if !event::is_key_event(&event) {
                let info = filter.check_event(&event, debounce_time, false);
                prop_assert!(
                    !info.is_bounce,
                    "Non-key event type:{event_type} code:{code} val:{value} at {event_us}us was incorrectly marked as bounce.",
                    event_us = info.event_us
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

        for (event_us, event_type, code, value) in event_data {
            let event = build_event(event_us, event_type, code, value);

            if event::is_key_event(&event) && event.value == 2 {
                let info = filter.check_event(&event, debounce_time, false);
                prop_assert!(
                    !info.is_bounce,
                    "Repeat event type:{event_type} code:{code} val:{value} at {event_us}us was incorrectly marked as bounce.",
                    event_us = info.event_us
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

        for (event_us, event_type, code, value) in &event_data {
            let event = build_event(*event_us, *event_type, *code, *value);
            let info = filter.check_event(&event, debounce_time, false);
            if !info.is_bounce {
                passed_events_ts.push(info.event_us);
            }
        }

        let mut last_ts = 0u64;
        for ts in passed_events_ts {
            prop_assert!(ts >= last_ts, "Passed event timestamps are not non-decreasing: {last_ts} followed by {ts}");
            last_ts = ts;
        }
    }

    /// Property: Internal counts in StatsCollector should be consistent.
    #[test]
    fn prop_stats_collector_consistency(
        event_data in arb_event_sequence_data(),
        debounce_ms in 0u64..=MAX_DEBOUNCE_MS,
        near_miss_ms in 0u64..=MAX_DEBOUNCE_MS
    ) {
        let debounce_time = Duration::from_millis(debounce_ms);
        let near_miss_threshold = Duration::from_millis(near_miss_ms);
        let config = dummy_config_no_arc(debounce_time, near_miss_threshold);

        let mut filter = BounceFilter::new(0);
        let mut stats = StatsCollector::with_capacity();

        for (event_us, event_type, code, value) in event_data {
            let event = build_event(event_us, event_type, code, value);
            let info = filter.check_event(&event, debounce_time, false);
            stats.record_event_info_with_config(&info, &config);
        }

        for key_code_idx in 0..FILTER_MAP_SIZE {
            let key_stats = &stats.per_key_stats[key_code_idx];

            prop_assert_eq!(
                key_stats.press.total_processed,
                key_stats.press.passed_count + key_stats.press.dropped_count,
                "Stats inconsistency for key {} (Press)", key_code_idx
            );
            prop_assert_eq!(
                key_stats.release.total_processed,
                key_stats.release.passed_count + key_stats.release.dropped_count,
                "Stats inconsistency for key {} (Release)", key_code_idx
            );
            prop_assert_eq!(
                key_stats.repeat.total_processed,
                key_stats.repeat.passed_count + key_stats.repeat.dropped_count,
                "Stats inconsistency for key {} (Repeat)", key_code_idx
            );

            let press_summary = key_stats.press.bounce_summary.count();
            let release_summary = key_stats.release.bounce_summary.count();
            let repeat_summary = key_stats.repeat.bounce_summary.count();

            prop_assert!(
                press_summary >= key_stats.press.bounce_histogram.count,
                "Bounce histogram count exceeds summary for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                press_summary <= key_stats.press.dropped_count,
                "Bounce summary exceeds drop count for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                release_summary >= key_stats.release.bounce_histogram.count,
                "Bounce histogram count exceeds summary for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                release_summary <= key_stats.release.dropped_count,
                "Bounce summary exceeds drop count for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                repeat_summary >= key_stats.repeat.bounce_histogram.count,
                "Bounce histogram count exceeds summary for key {} (Repeat)",
                key_code_idx
            );
            prop_assert!(
                repeat_summary <= key_stats.repeat.dropped_count,
                "Bounce summary exceeds drop count for key {} (Repeat)",
                key_code_idx
            );

            prop_assert!(
                key_stats.press.bounce_samples.len() as u64 <= press_summary,
                "Sample size exceeds summary count for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                key_stats.release.bounce_samples.len() as u64 <= release_summary,
                "Sample size exceeds summary count for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                key_stats.repeat.bounce_samples.len() as u64 <= repeat_summary,
                "Sample size exceeds summary count for key {} (Repeat)",
                key_code_idx
            );

            prop_assert!(
                key_stats.press.bounce_samples.len() <= MAX_BOUNCE_TIMING_SAMPLES,
                "Sample ring exceeded capacity for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                key_stats.release.bounce_samples.len() <= MAX_BOUNCE_TIMING_SAMPLES,
                "Sample ring exceeded capacity for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                key_stats.repeat.bounce_samples.len() <= MAX_BOUNCE_TIMING_SAMPLES,
                "Sample ring exceeded capacity for key {} (Repeat)",
                key_code_idx
            );

            let near_miss_press_idx = key_code_idx * NUM_KEY_STATES + 1;
            let near_miss_release_idx = key_code_idx * NUM_KEY_STATES;
            let near_miss_repeat_idx = key_code_idx * NUM_KEY_STATES + 2;

            let summary_press = &stats.per_key_near_miss_stats[near_miss_press_idx].summary;
            let summary_release = &stats.per_key_near_miss_stats[near_miss_release_idx].summary;
            let summary_repeat = &stats.per_key_near_miss_stats[near_miss_repeat_idx].summary;

            prop_assert!(
                summary_press.count() >= stats.per_key_near_miss_stats[near_miss_press_idx].histogram.count,
                "Near-miss histogram count exceeds summary for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                summary_press.count() <= key_stats.press.passed_count,
                "Near-miss summary exceeds passed count for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                summary_release.count() >= stats.per_key_near_miss_stats[near_miss_release_idx].histogram.count,
                "Near-miss histogram count exceeds summary for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                summary_release.count() <= key_stats.release.passed_count,
                "Near-miss summary exceeds passed count for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                summary_repeat.count() >= stats.per_key_near_miss_stats[near_miss_repeat_idx].histogram.count,
                "Near-miss histogram count exceeds summary for key {} (Repeat)",
                key_code_idx
            );
            prop_assert!(
                summary_repeat.count() <= key_stats.repeat.passed_count,
                "Near-miss summary exceeds passed count for key {} (Repeat)",
                key_code_idx
            );

            prop_assert!(
                stats.per_key_near_miss_stats[near_miss_press_idx].samples.len() as u64
                    <= summary_press.count(),
                "Near-miss sample count exceeds summary for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                stats.per_key_near_miss_stats[near_miss_release_idx].samples.len() as u64
                    <= summary_release.count(),
                "Near-miss sample count exceeds summary for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                stats.per_key_near_miss_stats[near_miss_repeat_idx].samples.len() as u64
                    <= summary_repeat.count(),
                "Near-miss sample count exceeds summary for key {} (Repeat)",
                key_code_idx
            );

            prop_assert!(
                stats.per_key_near_miss_stats[near_miss_press_idx].samples.len()
                    <= MAX_NEAR_MISS_TIMING_SAMPLES,
                "Near-miss sample ring exceeded capacity for key {} (Press)",
                key_code_idx
            );
            prop_assert!(
                stats.per_key_near_miss_stats[near_miss_release_idx].samples.len()
                    <= MAX_NEAR_MISS_TIMING_SAMPLES,
                "Near-miss sample ring exceeded capacity for key {} (Release)",
                key_code_idx
            );
            prop_assert!(
                stats.per_key_near_miss_stats[near_miss_repeat_idx].samples.len()
                    <= MAX_NEAR_MISS_TIMING_SAMPLES,
                "Near-miss sample ring exceeded capacity for key {} (Repeat)",
                key_code_idx
            );
        }

        let total_processed_overall: u64 = stats.per_key_stats.iter().map(|s| s.press.total_processed + s.release.total_processed + s.repeat.total_processed).sum();
        let total_passed_overall: u64 = stats.per_key_stats.iter().map(|s| s.press.passed_count + s.release.passed_count + s.repeat.passed_count).sum();
        let total_dropped_overall: u64 = stats.per_key_stats.iter().map(|s| s.press.dropped_count + s.release.dropped_count + s.repeat.dropped_count).sum();

        prop_assert_eq!(stats.key_events_processed, total_processed_overall);
        prop_assert_eq!(stats.key_events_passed, total_passed_overall);
        prop_assert_eq!(stats.key_events_dropped, total_dropped_overall);
        prop_assert_eq!(
            stats.key_events_processed,
            stats.key_events_passed + stats.key_events_dropped,
            "Overall processed vs passed+dropped mismatch"
        );

        stats.aggregate_histograms();
        let total_bounce_hist_count: u64 = stats.per_key_stats.iter().map(|s| s.press.bounce_histogram.count + s.release.bounce_histogram.count + s.repeat.bounce_histogram.count).sum();
        let total_near_miss_hist_count: u64 = stats.per_key_near_miss_stats.iter().map(|s| s.histogram.count).sum();

        prop_assert_eq!(stats.overall_bounce_histogram.count, total_bounce_hist_count);
        prop_assert_eq!(stats.overall_near_miss_histogram.count, total_near_miss_hist_count);
    }
}
