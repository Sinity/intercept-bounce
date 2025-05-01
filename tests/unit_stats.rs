//! Unit tests for the StatsCollector logic.
//! These tests focus on verifying the accumulation of statistics based on
//! EventInfo messages, simulating what the logger thread would do.

use intercept_bounce::filter::stats::StatsCollector;
use intercept_bounce::logger::EventInfo; // Use EventInfo from logger
use input_linux_sys::{input_event, timeval, EV_KEY, EV_SYN}; // Use event types

// --- Test Constants ---
const KEY_A: u16 = 30;
const KEY_B: u16 = 48;
const KEY_C: u16 = 46;
const DEBOUNCE_US: u64 = 10_000; // 10ms

// --- Test Helpers ---

/// Creates an EV_KEY input_event.
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

/// Creates an EV_SYN input_event.
fn syn_ev(ts_us: u64) -> input_event {
     input_event {
        time: timeval {
            tv_sec: (ts_us / 1_000_000) as i64,
            tv_usec: (ts_us % 1_000_000) as i64,
        },
        type_: EV_SYN as u16,
        code: 0, // SYN_REPORT
        value: 0,
    }
}

/// Creates an EventInfo struct simulating a passed event.
fn passed_event_info(event: input_event, event_us: u64, last_passed_us: Option<u64>) -> EventInfo {
    EventInfo {
        event,
        event_us,
        is_bounce: false,
        diff_us: None, // No diff if not a bounce
        last_passed_us,
    }
}

/// Creates an EventInfo struct simulating a bounced (dropped) event.
fn bounced_event_info(
    event: input_event,
    event_us: u64,
    diff_us: u64,
    last_passed_us: Option<u64>, // Include last passed time even for bounces
) -> EventInfo {
    EventInfo {
        event,
        event_us,
        is_bounce: true,
        diff_us: Some(diff_us),
        last_passed_us,
    }
}


// --- Test Cases ---

#[test]
fn stats_basic_counts() {
    let mut stats = StatsCollector::with_capacity();
    // Sequence: A press (pass), A press (bounce), A release (pass), A release (bounce), B press (pass)
    let ev1 = key_ev(1000, KEY_A, 1);
    let ev2 = key_ev(2000, KEY_A, 1); // Bounce of ev1 (diff 1000)
    let ev3 = key_ev(3000, KEY_A, 0); // Assume first release (pass)
    let ev4 = key_ev(4000, KEY_A, 0); // Bounce of ev3 (diff 1000)
    let ev5 = key_ev(5000, KEY_B, 1); // First B press (pass)

    stats.record_event_info(&passed_event_info(ev1, 1000, None));
    stats.record_event_info(&bounced_event_info(ev2, 2000, 1000, Some(1000)));
    stats.record_event_info(&passed_event_info(ev3, 3000, None));
    stats.record_event_info(&bounced_event_info(ev4, 4000, 1000, Some(3000)));
    stats.record_event_info(&passed_event_info(ev5, 5000, None));

    assert_eq!(stats.key_events_processed, 5);
    assert_eq!(stats.key_events_passed, 3); // ev1, ev3, ev5 passed
    assert_eq!(stats.key_events_dropped, 2); // ev2, ev4 dropped

    // Check stats for KEY_A
    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    assert_eq!(key_a_stats.press.count, 1); // ev2 dropped
    assert_eq!(key_a_stats.press.timings_us, vec![1000]);
    assert_eq!(key_a_stats.release.count, 1); // ev4 dropped
    assert_eq!(key_a_stats.release.timings_us, vec![1000]);
    assert_eq!(key_a_stats.repeat.count, 0);

    // Check stats for KEY_B
    let key_b_stats = &stats.per_key_stats[KEY_B as usize];
    assert_eq!(key_b_stats.press.count, 0); // ev5 passed
    assert_eq!(key_b_stats.release.count, 0);
    assert_eq!(key_b_stats.repeat.count, 0);
}

#[test]
fn stats_near_miss() {
    let mut stats = StatsCollector::with_capacity();
    // Near miss threshold is 100ms (100_000 us)
    let near_miss_time1 = DEBOUNCE_US + 500; // 10.5ms (near miss relative to event at 0)
    let near_miss_time2 = DEBOUNCE_US + 90_000; // 100ms (near miss relative to previous)
    let far_time = DEBOUNCE_US + 100_001; // 110.001ms (not near miss relative to previous)
    let bounce_time = DEBOUNCE_US - 1; // 9.999ms (bounce)

    let ev1_ts = 0;
    let ev2_ts = ev1_ts + near_miss_time1;
    let ev3_ts = ev2_ts + near_miss_time2;
    let ev4_ts = ev3_ts + far_time;
    let ev5_ts = ev4_ts + bounce_time;

    let ev1 = key_ev(ev1_ts, KEY_A, 1);
    let ev2 = key_ev(ev2_ts, KEY_A, 1); // Near miss 1
    let ev3 = key_ev(ev3_ts, KEY_A, 1); // Near miss 2
    let ev4 = key_ev(ev4_ts, KEY_A, 1); // Far
    let ev5 = key_ev(ev5_ts, KEY_A, 1); // Bounce

    stats.record_event_info(&passed_event_info(ev1, ev1_ts, None));
    stats.record_event_info(&passed_event_info(ev2, ev2_ts, Some(ev1_ts))); // last_passed = ev1_ts
    stats.record_event_info(&passed_event_info(ev3, ev3_ts, Some(ev2_ts))); // last_passed = ev2_ts
    stats.record_event_info(&passed_event_info(ev4, ev4_ts, Some(ev3_ts))); // last_passed = ev3_ts
    stats.record_event_info(&bounced_event_info(ev5, ev5_ts, bounce_time, Some(ev4_ts))); // last_passed = ev4_ts

    assert_eq!(stats.key_events_processed, 5);
    assert_eq!(stats.key_events_passed, 4);
    assert_eq!(stats.key_events_dropped, 1);

    // Check near miss stats for KEY_A, value 1 (press)
    let near_miss_idx = KEY_A as usize * 3 + 1;
    let near_misses = &stats.per_key_passed_near_miss_timing[near_miss_idx];
    assert_eq!(near_misses.len(), 2);
    // Near miss diffs are relative to the *previous passed* event
    assert_eq!(near_misses[0], near_miss_time1); // ev2 diff relative to ev1
    assert_eq!(near_misses[1], near_miss_time2); // ev3 diff relative to ev2
    // ev4 is not a near miss relative to ev3 (> 100ms)

    // Check bounce stats
    let key_a_stats = &stats.per_key_stats[KEY_A as usize];
    assert_eq!(key_a_stats.press.count, 1);
    assert_eq!(key_a_stats.press.timings_us, vec![bounce_time]);
}

#[test]
fn stats_ignores_non_key_events() {
     let mut stats = StatsCollector::with_capacity();
     let ev1 = key_ev(1000, KEY_A, 1);
     let ev2 = syn_ev(2000); // SYN event

     // Simulate EventInfo for SYN (is_bounce=false, no last_passed)
     let syn_info = EventInfo { event: ev2, event_us: 2000, is_bounce: false, diff_us: None, last_passed_us: None };

     stats.record_event_info(&passed_event_info(ev1, 1000, None));
     stats.record_event_info(&syn_info); // Process the SYN event info

     assert_eq!(stats.key_events_processed, 1); // Only ev1 counted
     assert_eq!(stats.key_events_passed, 1);
     assert_eq!(stats.key_events_dropped, 0);
}


#[test]
fn stats_json_output_structure() {
    // Test that the JSON output structure is generally correct.
    // Doesn't validate exact values deeply, just presence of keys.
    let mut stats = StatsCollector::with_capacity();
    let ev1 = key_ev(1000, KEY_A, 1);
    let ev2 = key_ev(1500, KEY_A, 1); // Bounce (diff 500)
    let ev3 = key_ev(DEBOUNCE_US + 2000, KEY_A, 1); // Near miss (diff DEBOUNCE_US+1000)

    stats.record_event_info(&passed_event_info(ev1, 1000, None));
    stats.record_event_info(&bounced_event_info(ev2, 1500, 500, Some(1000)));
    stats.record_event_info(&passed_event_info(ev3, DEBOUNCE_US + 2000, Some(1000))); // Near miss relative to ev1

    let mut buf = Vec::new();
    // Provide dummy config values and runtime for the JSON call.
    stats.print_stats_json(DEBOUNCE_US, true, false, 0, Some(DEBOUNCE_US + 1000), &mut buf);
    let s = String::from_utf8(buf).unwrap();
    println!("JSON Output:\n{}", s); // Print JSON for debugging if test fails

    // Basic structural checks
    assert!(s.contains("\"meta\":"));
    assert!(s.contains("\"debounce_time_us\":"));
    assert!(s.contains("\"runtime_us\":"));
    assert!(s.contains("\"stats\":"));
    assert!(s.contains("\"key_events_processed\":"));
    assert!(s.contains("\"key_events_passed\":"));
    assert!(s.contains("\"key_events_dropped\":"));
    assert!(s.contains("\"per_key_stats\":"));
    // Check if KEY_A (30) stats are present (since it had a bounce)
    assert!(s.contains("\"30\":"));
    assert!(s.contains("\"press\":"));
    assert!(s.contains("\"count\": 1")); // Bounce count for press
    assert!(s.contains("\"timings_us\": ["));
    assert!(s.contains("500")); // Check bounce timing value
    assert!(s.contains("\"per_key_passed_near_miss_timing\":"));
    // Check if near miss for KEY_A, value 1 is present
    assert!(s.contains("\"[30,1]\": ["));
    assert!(s.contains(&format!("{}", DEBOUNCE_US + 1000))); // Check near miss timing value

    // Ensure keys with no drops/near-misses are NOT present (e.g., key B=48)
    assert!(!s.contains("\"48\":"));
    assert!(!s.contains("\"[48,")); // Check no near miss for key 48
}

#[test]
fn stats_only_passed() {
    let mut stats = StatsCollector::with_capacity();
    let ev1 = key_ev(0, KEY_C, 1);
    let ev2 = key_ev(DEBOUNCE_US + 1, KEY_C, 0);
    let ev3 = key_ev((DEBOUNCE_US + 1) * 2, KEY_C, 1);

    stats.record_event_info(&passed_event_info(ev1, 0, None));
    stats.record_event_info(&passed_event_info(ev2, DEBOUNCE_US + 1, None));
    stats.record_event_info(&passed_event_info(ev3, (DEBOUNCE_US + 1) * 2, Some(0))); // Pass relative to ev1

    assert_eq!(stats.key_events_processed, 3);
    assert_eq!(stats.key_events_passed, 3);
    assert_eq!(stats.key_events_dropped, 0);

    // Check counts for KEY_C are zero as no events were dropped
    let key_c_stats = &stats.per_key_stats[KEY_C as usize];
    assert_eq!(key_c_stats.press.count, 0);
    assert_eq!(key_c_stats.release.count, 0);
    assert_eq!(key_c_stats.repeat.count, 0);

    // Check near miss (ev3 relative to ev1) - diff should be > DEBOUNCE_US
    let near_miss_idx = KEY_C as usize * 3 + 1;
    let near_misses = &stats.per_key_passed_near_miss_timing[near_miss_idx];
    let expected_diff = (DEBOUNCE_US + 1) * 2; // Diff between ev3 and ev1
    if expected_diff < 100_000 { // Only record if < 100ms
        assert_eq!(near_misses.len(), 1);
        assert_eq!(near_misses[0], expected_diff);
    } else {
         assert!(near_misses.is_empty());
    }
}

#[test]
fn stats_only_dropped() {
    let mut stats = StatsCollector::with_capacity();
    let ev1 = key_ev(0, KEY_B, 1); // Pass
    let ev2 = key_ev(100, KEY_B, 1); // Drop (diff 100)
    let ev3 = key_ev(200, KEY_B, 1); // Drop (diff 200 relative to ev1)

    stats.record_event_info(&passed_event_info(ev1, 0, None));
    stats.record_event_info(&bounced_event_info(ev2, 100, 100, Some(0)));
    stats.record_event_info(&bounced_event_info(ev3, 200, 200, Some(0))); // Still relative to last passed (ev1)

    assert_eq!(stats.key_events_processed, 3);
    assert_eq!(stats.key_events_passed, 1); // Only ev1 passed
    assert_eq!(stats.key_events_dropped, 2); // ev2, ev3 dropped

    // Check stats for KEY_B
    let key_b_stats = &stats.per_key_stats[KEY_B as usize];
    assert_eq!(key_b_stats.press.count, 2);
    assert_eq!(key_b_stats.release.count, 0);
    assert_eq!(key_b_stats.repeat.count, 0);
    // Timings should contain the diff_us values passed in EventInfo
    assert_eq!(key_b_stats.press.timings_us, vec![100, 200]);
}
