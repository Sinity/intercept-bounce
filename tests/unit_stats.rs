//! Unit tests for the StatsCollector logic.

use intercept_bounce::config::Config;
use intercept_bounce::filter::stats::StatsCollector;
use intercept_bounce::logger::EventInfo;
use serde_json::{json, Value};
use std::time::Duration;

// Use the dev-dependency crate for helpers
use test_helpers::*;
// --- Test Helpers ---

// --- Test Cases ---

#[test]
fn stats_basic_counts() {
    let mut stats = StatsCollector::with_capacity();
    let ev1 = key_ev(1000, KEY_A, 1); // Pass
    let ev2 = key_ev(2000, KEY_A, 1); // Bounce (diff 1000)
    let ev3 = key_ev(3000, KEY_A, 0); // Pass
    let ev4 = key_ev(4000, KEY_A, 0); // Bounce (diff 1000)
    let ev5 = key_ev(5000, KEY_B, 1); // Pass

    let config = dummy_config_no_arc(DEBOUNCE_TIME, Duration::from_millis(100));

    stats.record_event_info_with_config(&passed_event_info(ev1, 1000, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev2, 2000, 1000, Some(1000)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev3, 3000, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev4, 4000, 1000, Some(3000)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev5, 5000, None), &config);

    assert_eq!(stats.key_events_processed, 5);
    assert_eq!(stats.key_events_passed, 3); // ev1, ev3, ev5
    assert_eq!(stats.key_events_dropped, 2); // ev2, ev4

    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    assert_eq!(key_a_stats.press.total_processed, 2); // ev1, ev2
    assert_eq!(key_a_stats.press.passed_count, 1); // ev1
    assert_eq!(key_a_stats.press.count, 1); // ev2 dropped
    assert_eq!(key_a_stats.press.timings_us, vec![1000]);

    assert_eq!(key_a_stats.release.total_processed, 2); // ev3, ev4
    assert_eq!(key_a_stats.release.passed_count, 1); // ev3
    assert_eq!(key_a_stats.release.count, 1); // ev4 dropped
    assert_eq!(key_a_stats.release.timings_us, vec![1000]);

    assert_eq!(key_a_stats.repeat.total_processed, 0);
    assert_eq!(key_a_stats.repeat.passed_count, 0);
    assert_eq!(key_a_stats.repeat.count, 0);

    let key_b_stats = &stats.per_key_stats[KEY_B as usize];
    assert_eq!(key_b_stats.press.total_processed, 1); // ev5
    assert_eq!(key_b_stats.press.passed_count, 1); // ev5
    assert_eq!(key_b_stats.press.count, 0);
    assert_eq!(key_b_stats.release.total_processed, 0);
    assert_eq!(key_b_stats.release.passed_count, 0);
    assert_eq!(key_b_stats.release.count, 0);
    assert_eq!(key_b_stats.repeat.total_processed, 0);
    assert_eq!(key_b_stats.repeat.passed_count, 0);
    assert_eq!(key_b_stats.repeat.count, 0);
}

#[test]
fn stats_near_miss_default_threshold() {
    let mut stats = StatsCollector::with_capacity();
    let near_miss_threshold = Duration::from_millis(100); // Default: 100ms
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;
    let near_miss_threshold_us = near_miss_threshold.as_micros() as u64;

    // Timings relative to previous *passed* event
    let near_miss_diff1 = debounce_us + 500; // 10.5ms (near miss)
    let not_near_miss_diff2 = debounce_us + near_miss_threshold_us - 1; // 109.999ms (NOT a near miss, > 100ms threshold)
    let far_diff = debounce_us + near_miss_threshold_us; // 110ms (not near miss)
    let bounce_diff = debounce_us - 1; // 9.999ms (bounce)

    let ev1_ts = 0;
    let ev2_ts = ev1_ts + near_miss_diff1;
    let ev3_ts = ev2_ts + not_near_miss_diff2;
    let ev4_ts = ev3_ts + far_diff;
    let ev5_ts = ev4_ts + bounce_diff;

    let ev1 = key_ev(ev1_ts, KEY_A, 1); // Pass (ts=0)
    let ev2 = key_ev(ev2_ts, KEY_A, 1); // Pass (ts=10500, diff=10500 -> Near miss 1)
    let ev3 = key_ev(ev3_ts, KEY_A, 1); // Pass (ts=120499, diff=109999 -> NOT near miss)
    let ev4 = key_ev(ev4_ts, KEY_A, 1); // Pass (ts=230499, diff=110000 -> Far)
    let ev5 = key_ev(ev5_ts, KEY_A, 1); // Bounce (ts=240498, diff=9999 -> Bounce)

    let config = dummy_config_no_arc(DEBOUNCE_TIME, near_miss_threshold);

    stats.record_event_info_with_config(&passed_event_info(ev1, ev1_ts, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev2, ev2_ts, Some(ev1_ts)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev3, ev3_ts, Some(ev2_ts)), &config); // ev3 diff = 109999us > 100000us threshold
    stats.record_event_info_with_config(&passed_event_info(ev4, ev4_ts, Some(ev3_ts)), &config);
    stats.record_event_info_with_config(
        &bounced_event_info(ev5, ev5_ts, bounce_diff, Some(ev4_ts)),
        &config,
    );

    assert_eq!(stats.key_events_processed, 5);
    assert_eq!(stats.key_events_passed, 4);
    assert_eq!(stats.key_events_dropped, 1);

    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    assert_eq!(key_a_stats.press.total_processed, 5); // All 5 events for A,1
    assert_eq!(key_a_stats.press.passed_count, 4); // ev1, ev2, ev3, ev4
    assert_eq!(key_a_stats.press.count, 1); // ev5 dropped

    // Check near miss stats for KEY_A, value 1 (press).
    let near_miss_idx = KEY_A as usize * 3 + 1;
    let near_misses = &stats.per_key_passed_near_miss_timing[near_miss_idx];
    // Only ev2 should be a near miss (diff 10500us <= 100000us threshold).
    // ev3's diff (109999us) is > 100000us threshold.
    assert_eq!(near_misses.len(), 1, "Expected exactly 1 near miss");
    assert_eq!(
        near_misses[0], near_miss_diff1,
        "Expected near miss timing for ev2"
    ); // ev4 is not a near miss relative to ev3.

    // Check bounce stats.
    assert_eq!(key_a_stats.press.timings_us, vec![bounce_diff]);
}

#[test]
fn stats_near_miss_custom_threshold() {
    let mut stats = StatsCollector::with_capacity();
    let custom_threshold = Duration::from_millis(50);
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;

    let ev1_ts = 0;
    let diff1 = debounce_us + 1000; // 11ms (within 50ms threshold)
    let ev2_ts = ev1_ts + diff1;
    let diff2 = 40_000; // 40ms (within 50ms threshold)
    let ev3_ts = ev2_ts + diff2;
    let diff3 = 60_000; // 60ms (outside 50ms threshold)
    let ev4_ts = ev3_ts + diff3;

    let ev1 = key_ev(ev1_ts, KEY_A, 1); // Pass (ts=0)
    let ev2 = key_ev(ev2_ts, KEY_A, 1); // Pass (ts=11000, diff=11000 -> Near miss 1)
    let ev3 = key_ev(ev3_ts, KEY_A, 1); // Pass (ts=51000, diff=40000 -> Near miss 2)
    let ev4 = key_ev(ev4_ts, KEY_A, 1); // Pass (ts=111000, diff=60000 -> Far)

    let config = dummy_config_no_arc(DEBOUNCE_TIME, custom_threshold);

    stats.record_event_info_with_config(&passed_event_info(ev1, ev1_ts, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev2, ev2_ts, Some(ev1_ts)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev3, ev3_ts, Some(ev2_ts)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev4, ev4_ts, Some(ev3_ts)), &config);

    assert_eq!(stats.key_events_processed, 4);
    assert_eq!(stats.key_events_passed, 4);
    assert_eq!(stats.key_events_dropped, 0);

    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    assert_eq!(key_a_stats.press.total_processed, 4);
    assert_eq!(key_a_stats.press.passed_count, 4);
    assert_eq!(key_a_stats.press.count, 0);

    // Check near miss stats for KEY_A, value 1 (press).
    let near_miss_idx = KEY_A as usize * 3 + 1;
    let near_misses = &stats.per_key_passed_near_miss_timing[near_miss_idx];
    // ev2 (diff 11000us) and ev3 (diff 40000us) are within the 50000us threshold.
    // ev4 (diff 60000us) is outside.
    assert_eq!(near_misses.len(), 2, "Expected 2 near misses");
    assert_eq!(near_misses[0], diff1, "Expected near miss timing for ev2"); // Diff between ev2 and ev1
    assert_eq!(near_misses[1], diff2, "Expected near miss timing for ev3"); // Diff between ev3 and ev2
}

#[test]
fn stats_ignores_non_key_events() {
    let mut stats = StatsCollector::with_capacity();
    let ev1 = key_ev(1000, KEY_A, 1); // Key event
    let ev2 = non_key_ev(2000); // SYN event
    let syn_info = EventInfo {
        event: ev2,
        event_us: 2000,
        is_bounce: false, // Non-key events are never bounces
        diff_us: None,
        last_passed_us: None,
    };

    let config = dummy_config_no_arc(DEBOUNCE_TIME, Duration::from_millis(100));

    stats.record_event_info_with_config(&passed_event_info(ev1, 1000, None), &config);
    stats.record_event_info_with_config(&syn_info, &config);

    assert_eq!(stats.key_events_processed, 1); // Only ev1 should be counted
    assert_eq!(stats.key_events_passed, 1);
    assert_eq!(stats.key_events_dropped, 0);

    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    assert_eq!(key_a_stats.press.total_processed, 1);
    assert_eq!(key_a_stats.press.passed_count, 1);
    assert_eq!(key_a_stats.press.count, 0);
}

#[test]
fn stats_json_output_structure() {
    let mut stats = StatsCollector::with_capacity();
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;
    let ev1_ts = 1000;
    let ev2_ts = ev1_ts + 500; // Bounce (diff 500)
    let ev3_ts = ev1_ts + debounce_us + 2000; // Pass (diff 11000 relative to ev1)

    let ev1 = key_ev(ev1_ts, KEY_A, 1);
    let ev2 = key_ev(ev2_ts, KEY_A, 1);
    let ev3 = key_ev(ev3_ts, KEY_A, 1);

    let config = Config::new(
        DEBOUNCE_TIME,
        Duration::from_millis(100), // near_miss_threshold (100000us)
        Duration::ZERO,             // log_interval
        true,                       // log_all_events
        false,                      // log_bounces
        true,                       // stats_json (important for this test)
        false,                      // verbose
        "info".to_string(),         // log_filter
        None,                       // otel_endpoint
    );

    stats.record_event_info_with_config(&passed_event_info(ev1, ev1_ts, None), &config);
    stats.record_event_info_with_config(
        &bounced_event_info(ev2, ev2_ts, 500, Some(ev1_ts)),
        &config,
    );
    stats.record_event_info_with_config(&passed_event_info(ev3, ev3_ts, Some(ev1_ts)), &config); // ev3 diff = 11000us <= 100000us threshold -> Near miss

    let mut buf = Vec::new();
    let runtime_us = ev3_ts + 1000; // Example runtime
    stats.print_stats_json(&config, Some(runtime_us), "Cumulative", &mut buf);
    let s = String::from_utf8(buf).unwrap();
    println!("JSON Output:\n{s}"); // Print for debugging

    // Basic structural checks using serde_json::from_str for robustness
    let json_value: Value = serde_json::from_str(&s).expect("Failed to parse JSON output");

    assert_eq!(json_value["report_type"], "Cumulative");
    assert_eq!(json_value["runtime_us"], runtime_us);
    assert_eq!(json_value["key_events_processed"], 3);
    assert_eq!(json_value["key_events_passed"], 2); // ev1, ev3
    assert_eq!(json_value["key_events_dropped"], 1); // ev2

    // Check raw config values
    assert_eq!(json_value["debounce_time_us"], DEBOUNCE_TIME.as_micros() as u64);
    assert_eq!(json_value["near_miss_threshold_us"], Duration::from_millis(100).as_micros() as u64);
    assert_eq!(json_value["log_interval_us"], Duration::ZERO.as_micros() as u64);


    // Check per_key_stats array
    let per_key_stats = json_value["per_key_stats"]
        .as_array()
        .expect("per_key_stats is not an array");
    assert_eq!(per_key_stats.len(), 1); // Only KEY_A should have entries with drops or passes
    let key_a_stats = &per_key_stats[0];
    assert_eq!(key_a_stats["key_code"], KEY_A);
    assert_eq!(key_a_stats["key_name"], "KEY_A");
    assert_eq!(key_a_stats["total_processed"], 3); // ev1, ev2, ev3
    assert_eq!(key_a_stats["total_dropped"], 1); // ev2
    assert!((key_a_stats["drop_percentage"].as_f64().unwrap() - (1.0/3.0)*100.0).abs() < f64::EPSILON);

    // Check detailed stats within the key entry
    let detailed_stats = &key_a_stats["stats"];
    assert_eq!(detailed_stats["press"]["total_processed"], 3); // ev1, ev2, ev3
    assert_eq!(detailed_stats["press"]["passed_count"], 2); // ev1, ev3
    assert_eq!(detailed_stats["press"]["dropped_count"], 1); // ev2
    assert!((detailed_stats["press"]["drop_rate"].as_f64().unwrap() - (1.0/3.0)*100.0).abs() < f64::EPSILON);
    assert_eq!(detailed_stats["press"]["timings_us"], json!([500])); // Bounce timing

    assert_eq!(detailed_stats["release"]["total_processed"], 0);
    assert_eq!(detailed_stats["release"]["passed_count"], 0);
    assert_eq!(detailed_stats["release"]["dropped_count"], 0);
    assert!((detailed_stats["release"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(detailed_stats["release"]["timings_us"], json!([]));

    assert_eq!(detailed_stats["repeat"]["total_processed"], 0);
    assert_eq!(detailed_stats["repeat"]["passed_count"], 0);
    assert_eq!(detailed_stats["repeat"]["dropped_count"], 0);
    assert!((detailed_stats["repeat"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(detailed_stats["repeat"]["timings_us"], json!([]));


    // Check near_miss array
    let near_miss_stats = json_value["per_key_passed_near_miss_timing"]
        .as_array()
        .expect("near_miss is not an array");
    assert_eq!(near_miss_stats.len(), 1); // Only ev3 near miss
    let key_a_near_miss = &near_miss_stats[0];
    assert_eq!(key_a_near_miss["key_code"], KEY_A);
    assert_eq!(key_a_near_miss["key_value"], 1);
    assert_eq!(key_a_near_miss["key_name"], "KEY_A");
    assert_eq!(key_a_near_miss["value_name"], "Press");
    assert_eq!(key_a_near_miss["count"], 1);
    let expected_near_miss_diff = ev3_ts - ev1_ts; // 11000
    assert_eq!(
        key_a_near_miss["timings_us"],
        json!([expected_near_miss_diff])
    );
    assert_eq!(key_a_near_miss["min_us"], expected_near_miss_diff);
    assert_eq!(key_a_near_miss["avg_us"], expected_near_miss_diff);
    assert_eq!(key_a_near_miss["max_us"], expected_near_miss_diff);
}

#[test]
fn stats_only_passed() {
    let mut stats = StatsCollector::with_capacity();
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;
    let near_miss_threshold = Duration::from_millis(100); // 100000us
    let near_miss_threshold_us = near_miss_threshold.as_micros() as u64;

    let ev1_ts = 0;
    let ev2_ts = ev1_ts + debounce_us + 1; // Pass (Release)
    let diff3 = debounce_us + near_miss_threshold_us - 1; // 109_999us. Pass (Press, NOT near miss relative to ev1, > 100ms threshold)
    let ev3_ts = ev1_ts + diff3;

    let ev1 = key_ev(ev1_ts, KEY_C, 1);
    let ev2 = key_ev(ev2_ts, KEY_C, 0);
    let ev3 = key_ev(ev3_ts, KEY_C, 1);

    let config = dummy_config_no_arc(DEBOUNCE_TIME, near_miss_threshold);

    stats.record_event_info_with_config(&passed_event_info(ev1, ev1_ts, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev2, ev2_ts, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev3, ev3_ts, Some(ev1_ts)), &config);

    assert_eq!(stats.key_events_processed, 3);
    assert_eq!(stats.key_events_passed, 3);
    assert_eq!(stats.key_events_dropped, 0);

    // Check counts for KEY_C.
    let key_c_stats = &stats.per_key_stats[KEY_C as usize];
    assert_eq!(key_c_stats.press.total_processed, 2); // ev1, ev3
    assert_eq!(key_c_stats.press.passed_count, 2); // ev1, ev3
    assert_eq!(key_c_stats.press.count, 0);
    assert_eq!(key_c_stats.release.total_processed, 1); // ev2
    assert_eq!(key_c_stats.release.passed_count, 1); // ev2
    assert_eq!(key_c_stats.release.count, 0);
    assert_eq!(key_c_stats.repeat.total_processed, 0);
    assert_eq!(key_c_stats.repeat.passed_count, 0);
    assert_eq!(key_c_stats.repeat.count, 0);


    // Check near miss stats for KEY_C press (value 1).
    let near_miss_idx = KEY_C as usize * 3 + 1;
    let near_misses = &stats.per_key_passed_near_miss_timing[near_miss_idx];
    // ev3's diff relative to ev1 is 109999us, which is > near_miss_threshold (100000us).
    // Therefore, ev3 is NOT a near miss.
    assert_eq!(near_misses.len(), 0, "Expected 0 near misses");
}

#[test]
fn stats_only_dropped() {
    let mut stats = StatsCollector::with_capacity();
    let ev1_ts = 0;
    let diff2 = 100; // Bounce
    let ev2_ts = ev1_ts + diff2;
    let diff3 = 200; // Bounce (relative to ev1)
    let ev3_ts = ev1_ts + diff3;

    let ev1 = key_ev(ev1_ts, KEY_B, 1); // Pass
    let ev2 = key_ev(ev2_ts, KEY_B, 1); // Drop
    let ev3 = key_ev(ev3_ts, KEY_B, 1); // Drop

    let config = dummy_config_no_arc(DEBOUNCE_TIME, Duration::from_millis(100));

    stats.record_event_info_with_config(&passed_event_info(ev1, ev1_ts, None), &config);
    stats.record_event_info_with_config(
        &bounced_event_info(ev2, ev2_ts, diff2, Some(ev1_ts)),
        &config,
    );
    stats.record_event_info_with_config(
        &bounced_event_info(ev3, ev3_ts, diff3, Some(ev1_ts)),
        &config,
    );

    assert_eq!(stats.key_events_processed, 3);
    assert_eq!(stats.key_events_passed, 1); // Only ev1
    assert_eq!(stats.key_events_dropped, 2); // ev2, ev3

    // Check bounce stats for KEY_B press.
    let key_b_stats = &stats.per_key_stats[KEY_B as usize];
    assert_eq!(key_b_stats.press.total_processed, 3); // ev1, ev2, ev3
    assert_eq!(key_b_stats.press.passed_count, 1); // ev1
    assert_eq!(key_b_stats.press.count, 2); // ev2, ev3 dropped
    assert_eq!(key_b_stats.release.total_processed, 0);
    assert_eq!(key_b_stats.release.passed_count, 0);
    assert_eq!(key_b_stats.release.count, 0);
    assert_eq!(key_b_stats.repeat.total_processed, 0);
    assert_eq!(key_b_stats.repeat.passed_count, 0);
    assert_eq!(key_b_stats.repeat.count, 0);

    assert_eq!(key_b_stats.press.timings_us, vec![diff2, diff3]);
}

#[test]
fn stats_passed_counts_and_drop_rates() {
    let mut stats = StatsCollector::with_capacity();
    let config = dummy_config_no_arc(DEBOUNCE_TIME, Duration::from_millis(100));
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;

    // Sequence for KEY_A Press (1): Pass, Drop, Pass
    let ev_a1 = key_ev(0, KEY_A, 1); // Pass
    let ev_a2 = key_ev(debounce_us / 2, KEY_A, 1); // Drop
    let ev_a3 = key_ev(debounce_us * 2, KEY_A, 1); // Pass

    // Sequence for KEY_A Release (0): Pass, Drop, Drop
    let ev_a4 = key_ev(debounce_us * 3, KEY_A, 0); // Pass
    let ev_a5 = key_ev(debounce_us * 3 + debounce_us / 2, KEY_A, 0); // Drop
    let ev_a6 = key_ev(debounce_us * 3 + debounce_us / 2 + 1, KEY_A, 0); // Drop

    // Sequence for KEY_B Press (1): Pass only
    let ev_b1 = key_ev(debounce_us * 4, KEY_B, 1); // Pass

    stats.record_event_info_with_config(&passed_event_info(ev_a1, 0, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a2, debounce_us / 2, debounce_us / 2, Some(0)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev_a3, debounce_us * 2, Some(0)), &config); // Pass relative to ev_a1

    stats.record_event_info_with_config(&passed_event_info(ev_a4, debounce_us * 3, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a5, debounce_us * 3 + debounce_us / 2, debounce_us / 2, Some(debounce_us * 3)), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a6, debounce_us * 3 + debounce_us / 2 + 1, debounce_us / 2 + 1, Some(debounce_us * 3)), &config);

    stats.record_event_info_with_config(&passed_event_info(ev_b1, debounce_us * 4, None), &config);

    // --- Assertions ---
    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    let key_b_stats = &stats.per_key_stats[KEY_B as usize];

    // KEY_A Press (1)
    assert_eq!(key_a_stats.press.total_processed, 3, "KEY_A Press total");
    assert_eq!(key_a_stats.press.passed_count, 2, "KEY_A Press passed"); // ev_a1, ev_a3
    assert_eq!(key_a_stats.press.count, 1, "KEY_A Press dropped"); // ev_a2
    // Drop rate: 1 / 3 = 33.33...%

    // KEY_A Release (0)
    assert_eq!(key_a_stats.release.total_processed, 3, "KEY_A Release total");
    assert_eq!(key_a_stats.release.passed_count, 1, "KEY_A Release passed"); // ev_a4
    assert_eq!(key_a_stats.release.count, 2, "KEY_A Release dropped"); // ev_a5, ev_a6
    // Drop rate: 2 / 3 = 66.66...%

    // KEY_B Press (1)
    assert_eq!(key_b_stats.press.total_processed, 1, "KEY_B Press total");
    assert_eq!(key_b_stats.press.passed_count, 1, "KEY_B Press passed"); // ev_b1
    assert_eq!(key_b_stats.press.count, 0, "KEY_B Press dropped");
    // Drop rate: 0 / 1 = 0%

    // Overall Counts
    assert_eq!(stats.key_events_processed, 7);
    assert_eq!(stats.key_events_passed, 4); // a1, a3, a4, b1
    assert_eq!(stats.key_events_dropped, 3); // a2, a5, a6
}

