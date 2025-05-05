//! Unit tests for the StatsCollector logic.

use intercept_bounce::config::Config;
use intercept_bounce::filter::stats::{StatsCollector, TimingHistogram, NUM_HISTOGRAM_BUCKETS, HISTOGRAM_BUCKET_BOUNDARIES_MS};
use intercept_bounce::logger::EventInfo;
use serde_json::{json, Value};
use std::io::Cursor; // For capturing human-readable output
use std::time::Duration;

// Use the dev-dependency crate for helpers
use test_helpers::*;

// --- Test Cases ---

#[test]
fn timing_histogram_record() {
    let mut hist = TimingHistogram::default();
    let boundaries_ms = HISTOGRAM_BUCKET_BOUNDARIES_MS;

    hist.record(500); // 0.5ms -> <1ms bucket (0)
    hist.record(1000); // 1ms -> 1-2ms bucket (1)
    hist.record(1999); // 1.999ms -> 1-2ms bucket (1)
    hist.record(2000); // 2ms -> 2-4ms bucket (2)
    hist.record(3999); // 3.999ms -> 2-4ms bucket (2)
    hist.record(boundaries_ms[boundaries_ms.len() - 1] * 1000); // 128ms -> >=128ms bucket (8)
    hist.record(boundaries_ms[boundaries_ms.len() - 1] * 1000 + 1); // 128.001ms -> >=128ms bucket (8)

    assert_eq!(hist.count, 7);
    assert_eq!(hist.buckets[0], 1); // <1ms
    assert_eq!(hist.buckets[1], 2); // 1-2ms
    assert_eq!(hist.buckets[2], 2); // 2-4ms
    assert_eq!(hist.buckets[3], 0); // 4-8ms
    assert_eq!(hist.buckets[4], 0); // 8-16ms
    assert_eq!(hist.buckets[5], 0); // 16-32ms
    assert_eq!(hist.buckets[6], 0); // 32-64ms
    assert_eq!(hist.buckets[7], 0); // 64-128ms
    assert_eq!(hist.buckets[8], 2); // >=128ms
}

#[test]
fn timing_histogram_average() {
    let mut hist = TimingHistogram::default();
    hist.record(1000);
    hist.record(2000);
    hist.record(3000);
    assert_eq!(hist.count, 3);
    assert_eq!(hist.sum_us, 6000);
    assert_eq!(hist.average_us(), 2000);

    let hist2 = TimingHistogram::default();
    assert_eq!(hist2.count, 0);
    assert_eq!(hist2.sum_us, 0);
    assert_eq!(hist2.average_us(), 0);
}


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
    assert_eq!(key_a_stats.press.dropped_count, 1); // ev2 dropped
    assert_eq!(key_a_stats.press.timings_us, vec![1000]);

    assert_eq!(key_a_stats.release.total_processed, 2); // ev3, ev4
    assert_eq!(key_a_stats.release.passed_count, 1); // ev3
    assert_eq!(key_a_stats.release.dropped_count, 1); // ev4 dropped
    assert_eq!(key_a_stats.release.timings_us, vec![1000]);

    assert_eq!(key_a_stats.repeat.total_processed, 0);
    assert_eq!(key_a_stats.repeat.passed_count, 0);
    assert_eq!(key_a_stats.repeat.dropped_count, 0);

    let key_b_stats = &stats.per_key_stats[KEY_B as usize];
    assert_eq!(key_b_stats.press.total_processed, 1); // ev5
    assert_eq!(key_b_stats.press.passed_count, 1); // ev5
    assert_eq!(key_b_stats.press.dropped_count, 0);
    assert_eq!(key_b_stats.release.total_processed, 0);
    assert_eq!(key_b_stats.release.passed_count, 0);
    assert_eq!(key_b_stats.release.dropped_count, 0);
    assert_eq!(key_b_stats.repeat.total_processed, 0);
    assert_eq!(key_b_stats.repeat.passed_count, 0);
    assert_eq!(key_b_stats.repeat.dropped_count, 0);
}

#[test]
fn stats_near_miss_default_threshold() {
    let mut stats = StatsCollector::with_capacity();
    let near_miss_threshold = Duration::from_millis(100); // Default: 100ms
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;
    let near_miss_threshold_us = near_miss_threshold.as_micros() as u64;

    // Timings relative to previous *passed* event
    let near_miss_diff1 = debounce_us + 500; // 10.5ms (near miss)
    let not_near_miss_diff2 = debounce_us + near_miss_threshold_us; // 110ms (NOT a near miss, > 100ms threshold)
    let far_diff = debounce_us + near_miss_threshold_us + 1000; // 111ms (not near miss)
    let bounce_diff = debounce_us - 1; // 9.999ms (bounce)

    let ev1_ts = 0;
    let ev2_ts = ev1_ts + near_miss_diff1;
    let ev3_ts = ev2_ts + not_near_miss_diff2;
    let ev4_ts = ev3_ts + far_diff;
    let ev5_ts = ev4_ts + bounce_diff;

    let ev1 = key_ev(ev1_ts, KEY_A, 1); // Pass (ts=0)
    let ev2 = key_ev(ev2_ts, KEY_A, 1); // Pass (ts=10500, diff=10500 -> Near miss 1)
    let ev3 = key_ev(ev3_ts, KEY_A, 1); // Pass (ts=120500, diff=110000 -> NOT near miss)
    let ev4 = key_ev(ev4_ts, KEY_A, 1); // Pass (ts=231500, diff=111000 -> Far)
    let ev5 = key_ev(ev5_ts, KEY_A, 1); // Bounce (ts=241499, diff=9999 -> Bounce)

    let config = dummy_config_no_arc(DEBOUNCE_TIME, near_miss_threshold);

    stats.record_event_info_with_config(&passed_event_info(ev1, ev1_ts, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev2, ev2_ts, Some(ev1_ts)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev3, ev3_ts, Some(ev2_ts)), &config); // ev3 diff = 110000us >= 100000us threshold
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
    assert_eq!(key_a_stats.press.dropped_count, 1); // ev5 dropped

    // Check near miss stats for KEY_A, value 1 (press).
    let near_miss_idx = KEY_A as usize * 3 + 1;
    let near_misses_stats = &stats.per_key_near_miss_stats[near_miss_idx];
    // Only ev2 should be a near miss (diff 10500us <= 100000us threshold).
    // ev3's diff (110000us) is >= 100000us threshold.
    assert_eq!(near_misses_stats.timings_us.len(), 1, "Expected exactly 1 near miss");
    assert_eq!(
        near_misses_stats.timings_us[0], near_miss_diff1,
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
    assert_eq!(key_a_stats.press.dropped_count, 0);

    // Check near miss stats for KEY_A, value 1 (press).
    let near_miss_idx = KEY_A as usize * 3 + 1;
    let near_misses_stats = &stats.per_key_near_miss_stats[near_miss_idx];
    // ev2 (diff 11000us) and ev3 (diff 40000us) are within the 50000us threshold.
    // ev4 (diff 60000us) is outside.
    assert_eq!(near_misses_stats.timings_us.len(), 2, "Expected 2 near misses");
    assert_eq!(near_misses_stats.timings_us[0], diff1, "Expected near miss timing for ev2"); // Diff between ev2 and ev1
    assert_eq!(near_misses_stats.timings_us[1], diff2, "Expected near miss timing for ev3"); // Diff between ev3 and ev2
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
    assert_eq!(key_a_stats.press.dropped_count, 0);
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

    // Check bounce histogram for KEY_A Press
    let bounce_hist = &detailed_stats["press"]["bounce_histogram"];
    assert_eq!(bounce_hist["count"], 1);
    assert_eq!(bounce_hist["avg_us"], 500);
    assert!(bounce_hist["buckets"].is_array());
    assert_eq!(bounce_hist["buckets"].as_array().unwrap().len(), NUM_HISTOGRAM_BUCKETS);
    // Check the bucket for 500us (0.5ms) - should be the first bucket (<1ms)
    assert_eq!(bounce_hist["buckets"][0]["min_ms"], 0);
    assert_eq!(bounce_hist["buckets"][0]["max_ms"], HISTOGRAM_BUCKET_BOUNDARIES_MS[0]);
    assert_eq!(bounce_hist["buckets"][0]["count"], 1);


    assert_eq!(detailed_stats["release"]["total_processed"], 0);
    assert_eq!(detailed_stats["release"]["passed_count"], 0);
    assert_eq!(detailed_stats["release"]["dropped_count"], 0);
    assert!((detailed_stats["release"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(detailed_stats["release"]["timings_us"], json!([]));
    assert_eq!(detailed_stats["release"]["bounce_histogram"]["count"], 0);


    assert_eq!(detailed_stats["repeat"]["total_processed"], 0);
    assert_eq!(detailed_stats["repeat"]["passed_count"], 0);
    assert_eq!(detailed_stats["repeat"]["dropped_count"], 0);
    assert!((detailed_stats["repeat"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(detailed_stats["repeat"]["timings_us"], json!([]));
    assert_eq!(detailed_stats["repeat"]["bounce_histogram"]["count"], 0);


    // Check per_key_near_miss_stats array
    let near_miss_stats_array = json_value["per_key_near_miss_stats"]
        .as_array()
        .expect("per_key_near_miss_stats is not an array");
    assert_eq!(near_miss_stats_array.len(), 1); // Only ev3 near miss
    let key_a_near_miss = &near_miss_stats_array[0];
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

    // Check near miss histogram for KEY_A Press
    let near_miss_hist = &key_a_near_miss["near_miss_histogram"];
    assert_eq!(near_miss_hist["count"], 1);
    assert_eq!(near_miss_hist["avg_us"], expected_near_miss_diff);
    assert!(near_miss_hist["buckets"].is_array());
    assert_eq!(near_miss_hist["buckets"].as_array().unwrap().len(), NUM_HISTOGRAM_BUCKETS);
    // Check the bucket for 11000us (11ms) - should be the 8-16ms bucket (index 4)
    assert_eq!(near_miss_hist["buckets"][4]["min_ms"], 8);
    assert_eq!(near_miss_hist["buckets"][4]["max_ms"], 16);
    assert_eq!(near_miss_hist["buckets"][4]["count"], 1);


    // Check overall histograms
    let overall_bounce_hist = &json_value["overall_bounce_histogram"];
    assert_eq!(overall_bounce_hist["count"], 1); // Only ev2 bounce
    assert_eq!(overall_bounce_hist["avg_us"], 500);
    assert_eq!(overall_bounce_hist["buckets"][0]["count"], 1); // 500us is <1ms

    let overall_near_miss_hist = &json_value["overall_near_miss_histogram"];
    assert_eq!(overall_near_miss_hist["count"], 1); // Only ev3 near miss
    assert_eq!(overall_near_miss_hist["avg_us"], expected_near_miss_diff);
    assert_eq!(overall_near_miss_hist["buckets"][4]["count"], 1); // 11000us is 8-16ms
}

#[test]
fn stats_human_output_formatting() {
    let mut stats = StatsCollector::with_capacity();
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64; // 10000 us
    let near_miss_threshold = Duration::from_millis(50); // 50000 us

    // Sequence for KEY_A Press (1): Pass, Drop, Pass, Drop
    let ev_a1 = key_ev(0, KEY_A, 1); // Pass
    let ev_a2 = key_ev(debounce_us / 2, KEY_A, 1); // Drop (diff 5000)
    let ev_a3 = key_ev(debounce_us * 2, KEY_A, 1); // Pass (diff 20000 relative to ev_a1) -> Near miss (20000 <= 50000)
    let ev_a4 = key_ev(debounce_us * 2 + debounce_us / 4, KEY_A, 1); // Drop (diff 2500 relative to ev_a3)

    // Sequence for KEY_A Release (0): Pass, Drop, Drop
    let ev_a5 = key_ev(debounce_us * 3, KEY_A, 0); // Pass
    let ev_a6 = key_ev(debounce_us * 3 + debounce_us / 2, KEY_A, 0); // Drop (diff 5000 relative to ev_a5)
    let ev_a7 = key_ev(debounce_us * 3 + debounce_us / 2 + 1000, KEY_A, 0); // Drop (diff 6000 relative to ev_a5)

    // Sequence for KEY_B Press (1): Pass only, one near miss
    let ev_b1 = key_ev(debounce_us * 4, KEY_B, 1); // Pass
    let ev_b2 = key_ev(debounce_us * 4 + near_miss_threshold.as_micros() as u64 - 1, KEY_B, 1); // Pass (diff 49999 relative to ev_b1) -> Near miss

    let config = dummy_config_no_arc(DEBOUNCE_TIME, near_miss_threshold);

    stats.record_event_info_with_config(&passed_event_info(ev_a1, 0, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a2, debounce_us / 2, debounce_us / 2, Some(0)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev_a3, debounce_us * 2, Some(0)), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a4, debounce_us * 2 + debounce_us / 4, debounce_us / 4, Some(debounce_us * 2)), &config);

    stats.record_event_info_with_config(&passed_event_info(ev_a5, debounce_us * 3, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a6, debounce_us * 3 + debounce_us / 2, debounce_us / 2, Some(debounce_us * 3)), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a7, debounce_us * 3 + debounce_us / 2 + 1000, debounce_us / 2 + 1000, Some(debounce_us * 3)), &config);

    stats.record_event_info_with_config(&passed_event_info(ev_b1, debounce_us * 4, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev_b2, debounce_us * 4 + near_miss_threshold.as_micros() as u64 - 1, Some(debounce_us * 4)), &config);


    let mut writer = Cursor::new(Vec::new());
    stats.format_stats_human_readable(&config, "Cumulative", &mut writer).expect("Formatting failed");
    let output_string = String::from_utf8(writer.into_inner()).expect("Output not UTF-8");
    println!("Human Readable Output:\n{output_string}"); // Print for debugging

    // Overall Stats
    assert!(output_string.contains("--- Overall Statistics (Cumulative) ---"));
    assert!(output_string.contains("Key Events Processed: 9"));
    assert!(output_string.contains("Key Events Passed:   5")); // a1, a3, a5, b1, b2
    assert!(output_string.contains("Key Events Dropped:  4")); // a2, a4, a6, a7
    assert!(output_string.contains("Percentage Dropped:  44.44%")); // 4/9

    // Overall Bounce Histogram
    assert!(output_string.contains("--- Overall Bounce Timing Histogram ---"));
    // Check counts for bounces: 5000, 2500, 5000, 6000 us
    // 2500 us = 2.5 ms -> 2-4ms bucket (index 2)
    // 5000 us = 5.0 ms -> 4-8ms bucket (index 3) - two of these
    // 6000 us = 6.0 ms -> 4-8ms bucket (index 3) - one of these
    // Total: 1 in 2-4ms, 3 in 4-8ms. Total count 4.
    assert!(output_string.contains("2-4ms     : 1"));
    assert!(output_string.contains("4-8ms     : 3"));
    assert!(output_string.contains("Total: 4, Avg: 4.6 ms")); // (5000+2500+5000+6000)/4 = 18500/4 = 4625 us = 4.625 ms

    // Overall Near-Miss Histogram
    assert!(output_string.contains("--- Overall Near-Miss Timing Histogram (Passed within 50ms) ---"));
    // Check counts for near misses: 20000, 49999 us
    // 20000 us = 20 ms -> 16-32ms bucket (index 5)
    // 49999 us = 49.999 ms -> 32-64ms bucket (index 6)
    // Total: 1 in 16-32ms, 1 in 32-64ms. Total count 2.
    assert!(output_string.contains("16-32ms   : 1"));
    assert!(output_string.contains("32-64ms   : 1"));
    // Fix assertion for average near-miss time
    assert!(output_string.contains("Total: 2, Avg: 35.0 ms")); // (20000+49999)/2 = 34999.5 us = 35.0 ms

    // Per-Key Stats
    assert!(output_string.contains("--- Dropped Event Statistics Per Key ---"));
    assert!(output_string.contains("Key [KEY_A] (30):"));
    assert!(output_string.contains("Total Processed: 7, Passed: 3, Dropped: 4 (57.14%)")); // 4/7 = 57.14%
    assert!(output_string.contains("Press   (1): Processed: 4, Passed: 2, Dropped: 2 (50.00%)")); // 2/4 = 50%
    assert!(output_string.contains("Bounce Time: 2.5 ms / 3.8 ms / 5.0 ms")); // (2500+5000)/2 = 3750 us = 3.75 ms
    assert!(output_string.contains("Release (0): Processed: 3, Passed: 1, Dropped: 2 (66.67%)")); // 2/3 = 66.67%
    assert!(output_string.contains("Bounce Time: 5.0 ms / 5.5 ms / 6.0 ms")); // (5000+6000)/2 = 5500 us = 5.5 ms

    assert!(output_string.contains("Key [KEY_B] (48):"));
    assert!(output_string.contains("Total Processed: 2, Passed: 2, Dropped: 0 (0.00%)")); // 0/2 = 0%
    assert!(output_string.contains("Press   (1): Processed: 2, Passed: 2, Dropped: 0 (0.00%)")); // 0/2 = 0%

    // Check that the line for KEY_B Release is NOT present.
    // Find the section for KEY_B
    let key_b_section_start = output_string.find("Key [KEY_B] (48):");
    assert!(key_b_section_start.is_some(), "KEY_B section should be present");
    let rest_of_output = &output_string[key_b_section_start.unwrap()..];
    // Find the start of the next key section or the end of the output
    let next_key_start = rest_of_output[1..].find("\nKey ["); // Look for newline followed by "Key ["
    let key_b_section_end = next_key_start.map_or(rest_of_output.len(), |idx| idx + 1); // Adjust index
    let key_b_section = &rest_of_output[..key_b_section_end];

    // Assert that the Release line is NOT within the KEY_B section
    assert!(!key_b_section.contains("\n  Release (0):")); // Check for newline + indentation + "Release (0):"
    // Assert that the Repeat line is NOT within the KEY_B section
    assert!(!key_b_section.contains("\n  Repeat  (2):")); // Check for newline + indentation + "Repeat (2):"


    // Per-Key Near-Miss Stats
    assert!(output_string.contains("--- Passed Event Near-Miss Statistics (Passed within 50ms) ---"));
    assert!(output_string.contains("Key [KEY_A] (30, 1): 1 (Near-Miss Time: 20.0 ms / 20.0 ms / 20.0 ms)")); // ev_a3 diff 20000 us
    assert!(output_string.contains("Key [KEY_B] (48, 1): 1 (Near-Miss Time: 50.0 ms / 50.0 ms / 50.0 ms)")); // ev_b2 diff 49999 us (rounded to 50.0 ms)
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

    // Sequence for KEY_C Repeat (2): Pass only
    let ev_c1 = key_ev(debounce_us * 5, KEY_C, 2); // Pass

    stats.record_event_info_with_config(&passed_event_info(ev_a1, 0, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a2, debounce_us / 2, debounce_us / 2, Some(0)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev_a3, debounce_us * 2, Some(0)), &config); // Pass relative to ev_a1

    stats.record_event_info_with_config(&passed_event_info(ev_a4, debounce_us * 3, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a5, debounce_us * 3 + debounce_us / 2, debounce_us / 2, Some(debounce_us * 3)), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a6, debounce_us * 3 + debounce_us / 2 + 1, debounce_us / 2 + 1, Some(debounce_us * 3)), &config);

    stats.record_event_info_with_config(&passed_event_info(ev_b1, debounce_us * 4, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev_c1, debounce_us * 5, None), &config);


    // --- Assertions ---
    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    let key_b_stats = &stats.per_key_stats[KEY_B as usize];
    let key_c_stats = &stats.per_key_stats[KEY_C as usize];


    // KEY_A Press (1)
    assert_eq!(key_a_stats.press.total_processed, 3, "KEY_A Press total");
    assert_eq!(key_a_stats.press.passed_count, 2, "KEY_A Press passed"); // ev_a1, ev_a3
    assert_eq!(key_a_stats.press.dropped_count, 1, "KEY_A Press dropped"); // ev_a2
    // Drop rate: 1 / 3 = 33.33...%

    // KEY_A Release (0)
    assert_eq!(key_a_stats.release.total_processed, 3, "KEY_A Release total");
    assert_eq!(key_a_stats.release.passed_count, 1, "KEY_A Release passed"); // ev_a4
    assert_eq!(key_a_stats.release.dropped_count, 2, "KEY_A Release dropped"); // ev_a5, ev_a6
    // Drop rate: 2 / 3 = 66.66...%

    // KEY_B Press (1)
    assert_eq!(key_b_stats.press.total_processed, 1, "KEY_B Press total");
    assert_eq!(key_b_stats.press.passed_count, 1, "KEY_B Press passed"); // ev_b1
    assert_eq!(key_b_stats.press.dropped_count, 0, "KEY_B Press dropped");
    // Drop rate: 0 / 1 = 0%

    // KEY_C Repeat (2)
    assert_eq!(key_c_stats.repeat.total_processed, 1, "KEY_C Repeat total");
    assert_eq!(key_c_stats.repeat.passed_count, 1, "KEY_C Repeat passed"); // ev_c1
    assert_eq!(key_c_stats.repeat.dropped_count, 0, "KEY_C Repeat dropped");
    // Drop rate: 0 / 1 = 0%

    // Overall Counts
    // 3 (A Press) + 3 (A Release) + 1 (B Press) + 1 (C Repeat) = 8 events processed
    assert_eq!(stats.key_events_processed, 8);
    assert_eq!(stats.key_events_passed, 5); // a1, a3, a4, b1, c1
    assert_eq!(stats.key_events_dropped, 3); // a2, a5, a6
}

#[test]
fn stats_collector_aggregate_histograms() {
    let mut stats = StatsCollector::with_capacity();
    let config = dummy_config_no_arc(DEBOUNCE_TIME, Duration::from_millis(100));

    // Add bounces for KEY_A Press (diffs: 500, 1500 us)
    let ev_a1 = key_ev(0, KEY_A, 1); // Pass
    let ev_a2 = key_ev(500, KEY_A, 1); // Drop (diff 500)
    let ev_a3 = key_ev(2000, KEY_A, 1); // Drop (diff 1500 relative to ev_a1)
    stats.record_event_info_with_config(&passed_event_info(ev_a1, 0, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a2, 500, 500, Some(0)), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_a3, 2000, 1500, Some(0)), &config);

    // Add bounces for KEY_B Release (diffs: 3000 us)
    let ev_b1 = key_ev(10000, KEY_B, 0); // Pass
    let ev_b2 = key_ev(13000, KEY_B, 0); // Drop (diff 3000)
    stats.record_event_info_with_config(&passed_event_info(ev_b1, 10000, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_b2, 13000, 3000, Some(10000)), &config);

    // Add near misses for KEY_A Press (diffs: 21000, 25000 us) - Corrected diff for ev_a4
    let ev_a4 = key_ev(21000, KEY_A, 1); // Pass (diff 21000 relative to ev_a1 @ 0) -> Near miss
    let ev_a5 = key_ev(46000, KEY_A, 1); // Pass (diff 25000 relative to ev_a4 @ 21000) -> Near miss
    stats.record_event_info_with_config(&passed_event_info(ev_a4, 21000, Some(0)), &config);
    stats.record_event_info_with_config(&passed_event_info(ev_a5, 46000, Some(21000)), &config);

    // Add near misses for KEY_B Press (diffs: 40000 us)
    let ev_b3 = key_ev(50000, KEY_B, 1); // Pass
    let ev_b4 = key_ev(90000, KEY_B, 1); // Pass (diff 40000 relative to ev_b3 @ 50000) -> Near miss
    stats.record_event_info_with_config(&passed_event_info(ev_b3, 50000, None), &config);
    stats.record_event_info_with_config(&passed_event_info(ev_b4, 90000, Some(50000)), &config);


    // Aggregate the histograms
    stats.aggregate_histograms();

    // Check overall bounce histogram
    let overall_bounce_hist = &stats.overall_bounce_histogram;
    assert_eq!(overall_bounce_hist.count, 3); // 500, 1500, 3000 us
    assert_eq!(overall_bounce_hist.sum_us, 500 + 1500 + 3000); // 5000 us
    assert_eq!(overall_bounce_hist.average_us(), 5000 / 3); // 1666 us

    // 500 us = 0.5 ms -> <1ms bucket (0)
    // 1500 us = 1.5 ms -> 1-2ms bucket (1)
    // 3000 us = 3.0 ms -> 2-4ms bucket (2)
    assert_eq!(overall_bounce_hist.buckets[0], 1);
    assert_eq!(overall_bounce_hist.buckets[1], 1);
    assert_eq!(overall_bounce_hist.buckets[2], 1);
    assert_eq!(overall_bounce_hist.buckets[3], 0); // 4-8ms
    // ... other buckets should be 0

    // Check overall near-miss histogram
    let overall_near_miss_hist = &stats.overall_near_miss_histogram;
    assert_eq!(overall_near_miss_hist.count, 3); // 21000, 25000, 40000 us
    assert_eq!(overall_near_miss_hist.sum_us, 21000 + 25000 + 40000); // 86000 us
    assert_eq!(overall_near_miss_hist.average_us(), 86000 / 3); // 28666 us

    // 21000 us = 21 ms -> 16-32ms bucket (5)
    // 25000 us = 25 ms -> 16-32ms bucket (5)
    // 40000 us = 40 ms -> 32-64ms bucket (6)
    assert_eq!(overall_near_miss_hist.buckets[5], 2); // Corrected bucket index and count
    assert_eq!(overall_near_miss_hist.buckets[6], 1);
    assert_eq!(overall_near_miss_hist.buckets[0], 0); // <1ms
    // ... other buckets should be 0
}

#[test]
fn stats_only_passed() {
    let mut stats = StatsCollector::with_capacity();
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;
    let near_miss_threshold = Duration::from_millis(100); // 100000us
    let near_miss_threshold_us = near_miss_threshold.as_micros() as u64;

    let ev1_ts = 0;
    let ev2_ts = ev1_ts + debounce_us + 1; // Pass (Release)
    let diff3 = debounce_us + near_miss_threshold_us; // 110_000us. Pass (Press, NOT near miss relative to ev1, >= 100ms threshold)
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
    assert_eq!(key_c_stats.press.dropped_count, 0);
    assert_eq!(key_c_stats.release.total_processed, 1); // ev2
    assert_eq!(key_c_stats.release.passed_count, 1); // ev2
    assert_eq!(key_c_stats.release.dropped_count, 0);
    assert_eq!(key_c_stats.repeat.total_processed, 0);
    assert_eq!(key_c_stats.repeat.passed_count, 0);
    assert_eq!(key_c_stats.repeat.dropped_count, 0);


    // Check near miss stats for KEY_C press (value 1).
    let near_miss_idx = KEY_C as usize * 3 + 1;
    let near_misses_stats = &stats.per_key_near_miss_stats[near_miss_idx];
    // ev3's diff relative to ev1 is 110000us, which is >= near_miss_threshold (100000us).
    // Therefore, ev3 is NOT a near miss.
    assert_eq!(near_misses_stats.timings_us.len(), 0, "Expected 0 near misses");
    assert_eq!(near_misses_stats.histogram.count, 0, "Expected 0 near miss histogram counts");
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
    assert_eq!(key_b_stats.press.dropped_count, 2); // ev2, ev3 dropped
    assert_eq!(key_b_stats.release.total_processed, 0);
    assert_eq!(key_b_stats.release.passed_count, 0);
    assert_eq!(key_b_stats.release.dropped_count, 0);
    assert_eq!(key_b_stats.repeat.total_processed, 0);
    assert_eq!(key_b_stats.repeat.passed_count, 0);
    assert_eq!(key_b_stats.repeat.dropped_count, 0);

    assert_eq!(key_b_stats.press.timings_us, vec![diff2, diff3]);
    assert_eq!(key_b_stats.press.bounce_histogram.count, 2);
    // 100 us = 0.1 ms -> <1ms bucket (0)
    // 200 us = 0.2 ms -> <1ms bucket (0)
    assert_eq!(key_b_stats.press.bounce_histogram.buckets[0], 2);
}

#[test]
fn stats_drop_rate_edge_cases() {
    let mut stats = StatsCollector::with_capacity();
    let config = dummy_config_no_arc(DEBOUNCE_TIME, Duration::from_millis(100));
    let debounce_us = DEBOUNCE_TIME.as_micros() as u64;

    // Key A Press: 1 Pass, 0 Drop -> 0% drop rate
    let ev_a1 = key_ev(0, KEY_A, 1); // Pass
    stats.record_event_info_with_config(&passed_event_info(ev_a1, 0, None), &config);

    // Key B Press: 1 Pass, 1 Drop -> 50% drop rate
    let ev_b1 = key_ev(10000, KEY_B, 1); // Pass
    let ev_b2 = key_ev(10000 + debounce_us / 2, KEY_B, 1); // Drop
    stats.record_event_info_with_config(&passed_event_info(ev_b1, 10000, None), &config);
    stats.record_event_info_with_config(&bounced_event_info(ev_b2, 10000 + debounce_us / 2, debounce_us / 2, Some(10000)), &config);

    // Key C Press: 0 Pass, 1 Drop (shouldn't happen with current filter logic, but test stats calc)
    // Simulate this state directly for testing the calculation
    let key_c_stats = &mut stats.per_key_stats[KEY_C as usize].press;
    key_c_stats.total_processed = 1;
    key_c_stats.dropped_count = 1;
    // No passed_count, no timings_us, no histogram entry for this simulated case

    // Key D Press: 0 Processed -> 0% drop rate
    // Default state is already 0 processed, 0 dropped

    let mut writer = Cursor::new(Vec::new());
    stats.format_stats_human_readable(&config, "Cumulative", &mut writer).expect("Formatting failed");
    let output_string = String::from_utf8(writer.into_inner()).expect("Output not UTF-8");
    println!("Human Readable Output (Edge Cases):\n{output_string}"); // Print for debugging

    // Key A Press: 0%
    assert!(output_string.contains("Key [KEY_A] (30):"));
    assert!(output_string.contains("Press   (1): Processed: 1, Passed: 1, Dropped: 0 (0.00%)"));

    // Key B Press: 50%
    assert!(output_string.contains("Key [KEY_B] (48):"));
    assert!(output_string.contains("Press   (1): Processed: 2, Passed: 1, Dropped: 1 (50.00%)"));

    // Key C Press: 100% (simulated)
    assert!(output_string.contains("Key [KEY_C] (46):"));
    assert!(output_string.contains("Press   (1): Processed: 1, Passed: 0, Dropped: 1 (100.00%)"));

    // Key D should not appear in the per-key stats section as it had no activity recorded via record_event_info_with_config
    // The default state of the StatsCollector ensures this.

    // Check JSON output for edge cases
    let mut buf = Vec::new();
    stats.print_stats_json(&config, None, "Cumulative", &mut buf);
    let s = String::from_utf8(buf).unwrap();
    let json_value: Value = serde_json::from_str(&s).expect("Failed to parse JSON output");

    let per_key_stats = json_value["per_key_stats"].as_array().expect("per_key_stats is not an array");

    // Find and check KEY_A
    let key_a_json = per_key_stats.iter().find(|entry| entry["key_code"] == KEY_A).expect("KEY_A not found");
    assert_eq!(key_a_json["stats"]["press"]["total_processed"], 1);
    assert_eq!(key_a_json["stats"]["press"]["passed_count"], 1);
    assert_eq!(key_a_json["stats"]["press"]["dropped_count"], 0);
    assert!((key_a_json["stats"]["press"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);

    // Find and check KEY_B
    let key_b_json = per_key_stats.iter().find(|entry| entry["key_code"] == KEY_B).expect("KEY_B not found");
    assert_eq!(key_b_json["stats"]["press"]["total_processed"], 2);
    assert_eq!(key_b_json["stats"]["press"]["passed_count"], 1);
    assert_eq!(key_b_json["stats"]["press"]["dropped_count"], 1);
    assert!((key_b_json["stats"]["press"]["drop_rate"].as_f64().unwrap() - 50.0).abs() < f64::EPSILON);

    // Find and check KEY_C (simulated)
    let key_c_json = per_key_stats.iter().find(|entry| entry["key_code"] == KEY_C).expect("KEY_C not found");
    assert_eq!(key_c_json["stats"]["press"]["total_processed"], 1);
    assert_eq!(key_c_json["stats"]["press"]["passed_count"], 0);
    assert_eq!(key_c_json["stats"]["press"]["dropped_count"], 1);
    assert!((key_c_json["stats"]["press"]["drop_rate"].as_f64().unwrap() - 100.0).abs() < f64::EPSILON);

    // Ensure KEY_D is not present
    let key_d_present = per_key_stats.iter().any(|entry| entry["key_code"] == KEY_D as u16);
    assert!(!key_d_present, "KEY_D should not be in JSON stats because it had no activity");
}
