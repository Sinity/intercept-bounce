use assert_cmd::Command;
use input_linux_sys::input_event;
use predicates::prelude::*;
use serde_json::{json, Value};
use std::io::Write;
use std::mem::size_of;
use std::process::Output;

// Use the dev-dependency crate for helpers
use test_helpers::*;

// Helper to serialize events into bytes
fn events_to_bytes(events: &[input_event]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(events));
    for ev in events {
        // Safety: input_event is POD and the slice points to valid memory owned by ev.
        unsafe {
            bytes.write_all(std::slice::from_raw_parts(
                ev as *const _ as *const u8,
                size_of::<input_event>(),
            ))
        }
        .expect("Failed to write event to byte vector");
    }
    bytes
}

#[test]
fn drops_bounce() {
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce
    let input_events = vec![e1, e2];
    let expected_events = vec![e1]; // Only e1 should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);
    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output = cmd.output().expect("Failed to execute command");

    assert!(
        output.status.success(),
        "Command exited with non-zero status: {:?}",
        output.status
    );

    assert_eq!(
        output.stdout, expected_output_bytes,
        "Bounce event was not dropped"
    );
}

#[test]
fn passes_outside_window() {
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(6_000, KEY_A, 1); // Outside 5ms window
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // Both should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Event outside window was dropped"
    );
}

#[test]
fn passes_non_key_events() {
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = non_key_ev(1_000); // SYN event
    let e3 = key_ev(3_000, KEY_A, 1); // Bounce
    let e4 = non_key_ev(4_000); // SYN event
    let input_events = vec![e1, e2, e3, e4];
    let expected_events = vec![e1, e2, e4]; // Bounce e3 dropped, SYN events pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Non-key event was dropped or bounce was not filtered correctly"
    );
}

#[test]
fn filters_different_keys_independently() {
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(2_000, KEY_B, 1);
    let e3 = key_ev(3_000, KEY_A, 1); // Bounce of e1
    let e4 = key_ev(4_000, KEY_B, 1); // Bounce of e2
    let e5 = key_ev(6_000, KEY_A, 1); // Outside bounce window of e1
    let input_events = vec![e1, e2, e3, e4, e5];
    let expected_events = vec![e1, e2, e5]; // Bounces e3 and e4 dropped

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Filtering affected different keys incorrectly"
    );
}

#[test]
fn filters_key_release() {
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(1_000, KEY_A, 0);
    let e3 = key_ev(3_000, KEY_A, 0); // Bounce of e2
    let input_events = vec![e1, e2, e3];
    let expected_events = vec![e1, e2]; // Bounce e3 dropped

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Key release bounce was not filtered"
    );
}

#[test]
fn filters_key_repeat() {
    // Key repeats (value 2) are NOT debounced.
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(500_000, KEY_A, 2); // Repeat
    let e3 = key_ev(502_000, KEY_A, 2); // Repeat (would be bounce if repeats were debounced)
    let input_events = vec![e1, e2, e3];
    let expected_events = vec![e1, e2, e3]; // All should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Key repeat events should not be debounced"
    );
}

#[test]
fn window_zero_passes_all() {
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(1_000, KEY_A, 1); // Would be bounce with window > 1ms
    let e3 = key_ev(2_000, KEY_A, 0);
    let e4 = key_ev(3_000, KEY_A, 0); // Would be bounce if window > 1ms
    let input_events = vec![e1, e2, e3, e4];
    let expected_events = vec![e1, e2, e3, e4]; // All pass with window 0

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("0ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd
        .output()
        .expect("Failed to run command with 0ms debounce");
    assert!(output.status.success(), "Command failed with 0ms debounce");

    assert_eq!(
        output.stdout, expected_output_bytes,
        "Events were filtered when debounce window was 0ms"
    );
}

#[test]
fn handles_time_going_backwards() {
    let e1 = key_ev(5_000, KEY_A, 1); // @ 5ms
    let e2 = key_ev(3_000, KEY_A, 1); // @ 3ms (time jumped back)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // Both should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Event with earlier timestamp was dropped"
    );
}

#[test]
fn filters_just_below_window_boundary() {
    const WINDOW_MS: u64 = 10;
    let window_us = WINDOW_MS * 1_000;
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(window_us - 1, KEY_A, 1); // Just inside window (9.999ms)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1]; // e2 filtered

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg(format!("{WINDOW_MS}ms"))
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Event just inside window boundary was not filtered"
    );
}

#[test]
fn passes_at_window_boundary() {
    const WINDOW_MS: u64 = 10;
    let window_us = WINDOW_MS * 1_000;
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(window_us, KEY_A, 1); // Exactly at window boundary (10.000ms)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // e2 passes

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg(format!("{WINDOW_MS}ms"))
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Event exactly at window boundary was filtered"
    );
}

#[test]
fn test_complex_sequence() {
    const WINDOW_MS: u64 = 10;
    let window_us = WINDOW_MS * 1_000;

    let e1 = key_ev(0, KEY_A, 1); // Pass (A Press)
    let e2 = key_ev(window_us / 2, KEY_A, 1); // Bounce (A Press)
    let e3 = key_ev(window_us + 1, KEY_A, 0); // Pass (A Release)
    let e4 = key_ev(window_us + 1 + window_us / 2, KEY_A, 0); // Bounce (A Release)
    let e5 = non_key_ev(window_us * 2); // Pass (SYN)
    let e6 = key_ev(window_us * 2 + 1, KEY_B, 1); // Pass (B Press)
    let e7 = key_ev(window_us * 2 + 1 + window_us / 4, KEY_B, 2); // Pass (B Repeat)
    let e8 = key_ev(window_us * 3, KEY_A, 1); // Pass (A Press)
    let e9 = key_ev(window_us * 3 + window_us / 2, KEY_A, 1); // Bounce (A Press)
    let e10 = key_ev(window_us * 4, KEY_B, 2); // Pass (B Repeat)

    let input_events = vec![e1, e2, e3, e4, e5, e6, e7, e8, e9, e10];
    let expected_events = vec![e1, e3, e5, e6, e7, e8, e10]; // Bounces e2, e4, e9 dropped

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg(format!("{WINDOW_MS}ms"))
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();

    assert_eq!(
        output.stdout, expected_output_bytes,
        "Complex event sequence was not filtered correctly"
    );
}

#[test]
fn stats_output_human_readable() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce
    let e3 = key_ev(10_000, KEY_B, 1); // Pass
    let e4 = key_ev(12_000, KEY_B, 1); // Bounce
    let e5 = key_ev(100_000, KEY_A, 0); // Pass (Release)
    let input_events = vec![e1, e2, e3, e4, e5];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    cmd.assert()
        .success()
        .stderr(predicate::str::contains(
            "--- Overall Statistics (Cumulative) ---",
        ))
        .stderr(predicate::str::contains("Key Events Processed: 5"))
        .stderr(predicate::str::contains("Key Events Passed:   3")) // e1, e3, e5
        .stderr(predicate::str::contains("Key Events Dropped:  2")) // e2, e4
        .stderr(predicate::str::contains("Key [KEY_A] (30):"))
        .stderr(predicate::str::contains("Press   (1): Processed: 2, Passed: 1, Dropped: 1 (50.00%)")) // Check detail line for A press
        .stderr(predicate::str::contains(
            "Bounce Time: 3.0 ms / 3.0 ms / 3.0 ms", // Timing for e2
        ))
        .stderr(predicate::str::contains("Key [KEY_B] (48):"))
        .stderr(predicate::str::contains("Press   (1): Processed: 2, Passed: 1, Dropped: 1 (50.00%)")) // Check detail line for B press
        .stderr(predicate::str::contains(
            "Bounce Time: 2.0 ms / 2.0 ms / 2.0 ms", // Timing for e4
        ));
}

#[test]
fn stats_output_json() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce
    let input_events = vec![e1, e2];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--stats-json")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output = cmd.output().expect("Failed to run command");
    assert!(output.status.success());

    let stderr_str = String::from_utf8(output.stderr).expect("Stderr not valid UTF-8");

    // Find the start of the JSON block and parse from there.
    let json_start_index = stderr_str
        .find('{')
        .expect("No JSON block start '{' found in stderr");
    let json_part = &stderr_str[json_start_index..];

    let stats_json: Value = serde_json::from_str(json_part).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON from stderr: {e}\nStderr:\n{stderr_str}"
        )
    });

    assert_eq!(stats_json["report_type"], "Cumulative");
    assert_eq!(stats_json["key_events_processed"], 2);
    assert_eq!(stats_json["key_events_passed"], 1);
    assert_eq!(stats_json["key_events_dropped"], 1);

    // Assert raw config values
    assert_eq!(stats_json["debounce_time_us"], 5000); // 5ms
    assert!(stats_json["near_miss_threshold_us"].is_u64()); // Check default value if needed
    assert!(stats_json["log_interval_us"].is_u64()); // Check default value if needed


    // Assert that per_key_stats is an array
    assert!(
        stats_json["per_key_stats"].is_array(),
        "per_key_stats should be an array"
    );

    // Find the object for KEY_A (30) in the array
    let key_a_stats = stats_json["per_key_stats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key_code"] == 30)
        .expect("Did not find stats object for key_code 30 in per_key_stats array");

    // Assertions on the found object
    assert!(key_a_stats.is_object());
    assert_eq!(key_a_stats["key_name"], "KEY_A");
    assert_eq!(key_a_stats["total_processed"], 2); // e1, e2
    assert_eq!(key_a_stats["total_dropped"], 1); // e2
    assert!((key_a_stats["drop_percentage"].as_f64().unwrap() - 50.0).abs() < f64::EPSILON); // 1 drop / 2 processed = 50%

    // Check detailed stats within the key entry
    let detailed_stats = &key_a_stats["stats"];
    assert_eq!(detailed_stats["press"]["total_processed"], 2); // e1, e2
    assert_eq!(detailed_stats["press"]["passed_count"], 1); // e1 passed
    assert_eq!(detailed_stats["press"]["dropped_count"], 1); // e2 dropped
    assert!((detailed_stats["press"]["drop_rate"].as_f64().unwrap() - 50.0).abs() < f64::EPSILON); // 1 drop / 2 processed = 50%
    assert_eq!(detailed_stats["press"]["timings_us"], json!([3000])); // Bounce timing
    assert!(detailed_stats["press"]["bounce_histogram"].is_object());


    assert_eq!(detailed_stats["release"]["total_processed"], 0);
    assert_eq!(detailed_stats["release"]["passed_count"], 0);
    assert_eq!(detailed_stats["release"]["dropped_count"], 0);
    assert!((detailed_stats["release"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(detailed_stats["release"]["timings_us"], json!([]));
    assert!(detailed_stats["release"]["bounce_histogram"].is_object());


    assert_eq!(detailed_stats["repeat"]["total_processed"], 0);
    assert_eq!(detailed_stats["repeat"]["passed_count"], 0);
    assert_eq!(detailed_stats["repeat"]["dropped_count"], 0);
    assert!((detailed_stats["repeat"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(detailed_stats["repeat"]["timings_us"], json!([]));
    assert!(detailed_stats["repeat"]["bounce_histogram"].is_object());


    // Ensure KEY_B (48) is not present in the array
    let key_b_present = stats_json["per_key_stats"]
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["key_code"] == 48);
    assert!(
        !key_b_present,
        "Stats for key_code 48 should not be present"
    );

    // Check per_key_near_miss_stats array (should be empty)
    let near_miss_stats_array = stats_json["per_key_near_miss_stats"]
        .as_array()
        .expect("per_key_near_miss_stats is not an array");
    assert_eq!(near_miss_stats_array.len(), 0);

    // Check overall histograms (should be present but potentially empty counts)
    assert!(stats_json["overall_bounce_histogram"].is_object());
    assert_eq!(stats_json["overall_bounce_histogram"]["count"], 1); // e2 bounce
    assert!(stats_json["overall_near_miss_histogram"].is_object());
    assert_eq!(stats_json["overall_near_miss_histogram"]["count"], 0); // No near misses in this sequence
}

#[test]
fn log_bounces_flag() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce
    let input_events = vec![e1, e2];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--log-bounces")
        .env("RUST_LOG", "intercept_bounce=info") // Ensure info level is enabled
        .write_stdin(input_bytes);

    cmd.assert()
        .success()
        // Check for the DROP log line for the bounced event.
        .stderr(
            predicate::str::contains("[DROP]").and(predicate::str::contains("Key [KEY_A] (30)")),
        )
        // Ensure the PASS line for e1 is NOT present at info level without --log-all-events.
        .stderr(predicate::str::contains("[PASS]").not());
}

#[test]
fn log_all_events_flag() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce
    let e3 = non_key_ev(4_000); // SYN (Pass)
    let input_events = vec![e1, e2, e3];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--log-all-events")
        .env("RUST_LOG", "intercept_bounce=info") // Ensure info level is enabled
        .write_stdin(input_bytes);

    cmd.assert()
        .success()
        // Check for PASS log for e1.
        .stderr(
            predicate::str::contains("[PASS]").and(predicate::str::contains("Key [KEY_A] (30)")),
        )
        // Check for DROP log for e2.
        .stderr(
            predicate::str::contains("[DROP]").and(predicate::str::contains("Key [KEY_A] (30)")),
        )
        // Check that SYN events are NOT logged (only key events are logged).
        .stderr(predicate::str::contains("EV_SYN").not());
}

#[test]
fn test_debounce_zero_passes_all() {
    let e1 = key_ev(0, KEY_A, 1);
    let e2 = key_ev(1_000, KEY_A, 1); // Would bounce if window > 1ms
    let e3 = key_ev(2_000, KEY_A, 0);
    let e4 = key_ev(3_000, KEY_A, 0); // Would bounce if window > 1ms
    let input_events = vec![e1, e2, e3, e4];
    let expected_events = vec![e1, e2, e3, e4]; // All pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("0ms")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output: Output = cmd
        .output()
        .expect("Failed to run command with 0ms debounce");
    assert!(output.status.success(), "Command failed with 0ms debounce");

    assert_eq!(
        output.stdout, expected_output_bytes,
        "Events were filtered when debounce window was 0ms"
    );
}

#[test]
fn test_only_non_key_events() {
    let e1 = non_key_ev(1000);
    let e2 = non_key_ev(2000);
    let e3 = non_key_ev(3000);
    let input_events = vec![e1, e2, e3];
    let expected_events = vec![e1, e2, e3]; // All pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--stats-json")
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output = cmd
        .output()
        .expect("Failed to run command with only non-key events");
    assert!(
        output.status.success(),
        "Command failed with only non-key events"
    );

    // Check stdout contains all input events.
    assert_eq!(
        output.stdout, expected_output_bytes,
        "Non-key events were filtered or modified"
    );

    // Check stderr stats. It should contain a JSON block, even if counts are zero.
    let stderr_str = String::from_utf8(output.stderr).expect("Stderr not valid UTF-8");

    // Find the start of the JSON block and parse from there.
    let json_start_index = stderr_str
        .find('{')
        .expect("No JSON block start '{' found in stderr for non-key event test");
    let json_part = &stderr_str[json_start_index..];

    let stats_json: Value = serde_json::from_str(json_part).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON from non-key event stderr: {e}\nStderr:\n{stderr_str}"
        )
    });

    // Assert that key event counts are zero.
    assert_eq!(
        stats_json["key_events_processed"], 0,
        "Processed count should be 0 for non-key events"
    );
    assert_eq!(
        stats_json["key_events_passed"], 0,
        "Passed count should be 0 for non-key events"
    );
    assert_eq!(
        stats_json["key_events_dropped"], 0,
        "Dropped count should be 0 for non-key events"
    );
    assert!(
        stats_json["per_key_stats"]
            .as_array() // Check if it's an array
            .map_or(true, |a| a.is_empty()), // Check if the array is empty or not an array
        "Per-key stats should be empty"
    );
    assert!(
        stats_json["per_key_near_miss_stats"]
            .as_array() // Check if it's an array
            .map_or(true, |a| a.is_empty()), // Check if the array is empty or not an array
        "Near-miss stats should be empty"
    );
    // Overall histograms should be present but empty
    assert!(stats_json["overall_bounce_histogram"].is_object());
    assert_eq!(stats_json["overall_bounce_histogram"]["count"], 0);
    assert!(stats_json["overall_near_miss_histogram"].is_object());
    assert_eq!(stats_json["overall_near_miss_histogram"]["count"], 0);
}

#[test]
fn stats_output_only_passed() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(10_000, KEY_A, 0); // Pass
    let e3 = key_ev(20_000, KEY_B, 1); // Pass
    let input_events = vec![e1, e2, e3];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--stats-json") // Test JSON output
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output = cmd.output().expect("Failed to run command");
    assert!(output.status.success());

    let stderr_str = String::from_utf8(output.stderr).expect("Stderr not valid UTF-8");
    let json_start_index = stderr_str.find('{').expect("No JSON block start '{' found");
    let json_part = &stderr_str[json_start_index..];
    let stats_json: Value = serde_json::from_str(json_part).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON from stderr: {e}\nStderr:\n{stderr_str}"
        )
    });

    assert_eq!(stats_json["key_events_processed"], 3);
    assert_eq!(stats_json["key_events_passed"], 3);
    assert_eq!(stats_json["key_events_dropped"], 0);

    // Check per_key_stats for KEY_A
    let key_a_stats = stats_json["per_key_stats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key_code"] == KEY_A)
        .expect("KEY_A stats not found");
    assert_eq!(key_a_stats["total_processed"], 2); // e1, e2
    assert_eq!(key_a_stats["total_dropped"], 0);
    assert!((key_a_stats["drop_percentage"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(key_a_stats["stats"]["press"]["total_processed"], 1); // e1
    assert_eq!(key_a_stats["stats"]["press"]["passed_count"], 1);
    assert_eq!(key_a_stats["stats"]["press"]["dropped_count"], 0);
    assert!((key_a_stats["stats"]["press"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(key_a_stats["stats"]["release"]["total_processed"], 1); // e2
    assert_eq!(key_a_stats["stats"]["release"]["passed_count"], 1);
    assert_eq!(key_a_stats["stats"]["release"]["dropped_count"], 0);
    assert!((key_a_stats["stats"]["release"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);

    // Check per_key_stats for KEY_B
    let key_b_stats = stats_json["per_key_stats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key_code"] == KEY_B)
        .expect("KEY_B stats not found");
    assert_eq!(key_b_stats["total_processed"], 1); // e3
    assert_eq!(key_b_stats["total_dropped"], 0);
    assert!((key_b_stats["drop_percentage"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);
    assert_eq!(key_b_stats["stats"]["press"]["total_processed"], 1); // e3
    assert_eq!(key_b_stats["stats"]["press"]["passed_count"], 1);
    assert_eq!(key_b_stats["stats"]["press"]["dropped_count"], 0);
    assert!((key_b_stats["stats"]["press"]["drop_rate"].as_f64().unwrap() - 0.0).abs() < f64::EPSILON);

    // Check overall histograms are empty
    assert_eq!(stats_json["overall_bounce_histogram"]["count"], 0);
    assert_eq!(stats_json["overall_near_miss_histogram"]["count"], 0);
}

#[test]
fn stats_output_only_dropped() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Drop
    let e3 = key_ev(4_000, KEY_A, 1); // Drop
    let input_events = vec![e1, e2, e3];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--stats-json") // Test JSON output
        .env("RUST_LOG", "warn")
        .write_stdin(input_bytes);

    let output = cmd.output().expect("Failed to run command");
    assert!(output.status.success());

    let stderr_str = String::from_utf8(output.stderr).expect("Stderr not valid UTF-8");
    let json_start_index = stderr_str.find('{').expect("No JSON block start '{' found");
    let json_part = &stderr_str[json_start_index..];
    let stats_json: Value = serde_json::from_str(json_part).unwrap_or_else(|e| {
        panic!(
            "Failed to parse JSON from stderr: {e}\nStderr:\n{stderr_str}"
        )
    });

    assert_eq!(stats_json["key_events_processed"], 3);
    assert_eq!(stats_json["key_events_passed"], 1); // e1
    assert_eq!(stats_json["key_events_dropped"], 2); // e2, e3

    // Check per_key_stats for KEY_A
    let key_a_stats = stats_json["per_key_stats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key_code"] == KEY_A)
        .expect("KEY_A stats not found");
    assert_eq!(key_a_stats["total_processed"], 3); // e1, e2, e3
    assert_eq!(key_a_stats["total_dropped"], 2); // e2, e3
    assert!((key_a_stats["drop_percentage"].as_f64().unwrap() - (2.0/3.0)*100.0).abs() < f64::EPSILON);
    assert_eq!(key_a_stats["stats"]["press"]["total_processed"], 3); // e1, e2, e3
    assert_eq!(key_a_stats["stats"]["press"]["passed_count"], 1); // e1
    assert_eq!(key_a_stats["stats"]["press"]["dropped_count"], 2); // e2, e3
    assert!((key_a_stats["stats"]["press"]["drop_rate"].as_f64().unwrap() - (2.0/3.0)*100.0).abs() < f64::EPSILON);
    assert_eq!(key_a_stats["stats"]["press"]["timings_us"], json!([3000, 4000])); // Diffs relative to e1

    // Check overall bounce histogram
    let overall_bounce_hist = &stats_json["overall_bounce_histogram"];
    assert_eq!(overall_bounce_hist["count"], 2);
    assert_eq!(overall_bounce_hist["avg_us"], (3000 + 4000) / 2); // 3500
    // 3000 us = 3ms -> 2-4ms bucket (index 2)
    // 4000 us = 4ms -> 4-8ms bucket (index 3)
    assert_eq!(overall_bounce_hist["buckets"][2]["count"], 1);
    assert_eq!(overall_bounce_hist["buckets"][3]["count"], 1);

    // Check overall near-miss histogram is empty
    assert_eq!(stats_json["overall_near_miss_histogram"]["count"], 0);
}
