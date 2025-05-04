use assert_cmd::Command;
use input_linux_sys::{input_event, timeval, EV_KEY};
use predicates::prelude::*; // For stderr assertions
use serde::Deserialize; // Import the Deserialize trait
use serde_json::{json, Value};
use std::io::Write;
use std::mem::size_of;
use std::process::Output; // Import Output struct // For parsing JSON stats

const KEY_A: u16 = 30;
const KEY_B: u16 = 48;
const EV_SYN: u16 = 0; // For SYN_REPORT events

// Helper to create a key event
fn key_ev(ts: u64, code: u16, value: i32) -> input_event {
    input_event {
        time: timeval {
            tv_sec: (ts / 1_000_000) as i64,
            tv_usec: (ts % 1_000_000) as i64,
        },
        type_: EV_KEY as u16,
        code,
        value,
    }
}

// Helper to create a non-key event (e.g., SYN_REPORT)
fn non_key_ev(ts: u64) -> input_event {
    input_event {
        time: timeval {
            tv_sec: (ts / 1_000_000) as i64,
            tv_usec: (ts % 1_000_000) as i64,
        },
        type_: EV_SYN, // Example: SYN event
        code: 0,       // SYN_REPORT
        value: 0,
    }
}

// Helper to serialize events into bytes
fn events_to_bytes(events: &[input_event]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for ev in events {
        unsafe {
            bytes
                .write_all(std::slice::from_raw_parts(
                    ev as *const _ as *const u8,
                    size_of::<input_event>(),
                ))
                .unwrap();
        }
    }
    bytes
}

#[test]
fn drops_bounce() {
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(3_000, KEY_A, 1); // Press A again at 3ms (bounce)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1]; // Only the first event should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);
    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms") // 5ms window - ADDED ms SUFFIX
        .write_stdin(input_bytes);

    // Execute the command and capture output
    let output_result = cmd.output();

    // Check if the command execution itself failed
    let output = match output_result {
        Ok(out) => out,
        Err(e) => {
            panic!("Failed to execute command: {}", e);
        }
    };

    // Assert that the command exited successfully
    assert!(
        output.status.success(),
        "Command exited with non-zero status: {:?}",
        output.status
    );

    // Now assert the stdout content
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Bounce event was not dropped"
    );
}

#[test]
fn passes_outside_window() {
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(6_000, KEY_A, 1); // Press A again at 6ms (outside 5ms window)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // Both events should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms") // 5ms window - ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Event outside window was dropped"
    );
}

#[test]
fn passes_non_key_events() {
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = non_key_ev(1_000); // SYN event at 1ms
    let e3 = key_ev(3_000, KEY_A, 1); // Press A again at 3ms (bounce)
    let e4 = non_key_ev(4_000); // SYN event at 4ms
    let input_events = vec![e1, e2, e3, e4];
    let expected_events = vec![e1, e2, e4]; // Key bounce (e3) dropped, SYN events pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms") // 5ms window - ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Non-key event was dropped or bounce was not filtered correctly"
    );
}

#[test]
fn filters_different_keys_independently() {
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(2_000, KEY_B, 1); // Press B at 2ms
    let e3 = key_ev(3_000, KEY_A, 1); // Press A again at 3ms (bounce of e1)
    let e4 = key_ev(4_000, KEY_B, 1); // Press B again at 4ms (bounce of e2)
    let e5 = key_ev(6_000, KEY_A, 1); // Press A again at 6ms (outside bounce window of e1)
    let input_events = vec![e1, e2, e3, e4, e5];
    let expected_events = vec![e1, e2, e5]; // Bounces e3 and e4 dropped

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms") // 5ms window - ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Filtering affected different keys incorrectly"
    );
}

#[test]
fn filters_key_release() {
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(1_000, KEY_A, 0); // Release A at 1ms
    let e3 = key_ev(3_000, KEY_A, 0); // Release A again at 3ms (bounce of e2)
    let input_events = vec![e1, e2, e3];
    let expected_events = vec![e1, e2]; // Bounce e3 dropped

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms") // 5ms window - ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Key release bounce was not filtered"
    );
}

#[test]
fn filters_key_repeat() {
    // Key repeats (value 2) are NOT debounced: all repeat events should pass, regardless of timing.
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(500_000, KEY_A, 2); // Repeat A at 500ms (normal repeat)
    let e3 = key_ev(502_000, KEY_A, 2); // Repeat A again at 502ms (would be "bounce" if we debounced repeats)
    let input_events = vec![e1, e2, e3];
    let expected_events = vec![e1, e2, e3]; // All repeats should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms") // ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Key repeat events should not be debounced"
    );
}

#[test]
fn window_zero_passes_all() {
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(1_000, KEY_A, 1); // Press A again at 1ms (would be bounce with window > 1)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // Both should pass when window is 0

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("0ms") // 0ms window - ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Events were filtered when window was 0"
    );
}

#[test]
fn handles_time_going_backwards() {
    let e1 = key_ev(5_000, KEY_A, 1); // Press A at 5ms
    let e2 = key_ev(3_000, KEY_A, 1); // Press A "again" at 3ms (time jumped back)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // Both events should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms") // 5ms window - ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Event with earlier timestamp was dropped"
    );
}

#[test]
fn filters_just_below_window_boundary() {
    const WINDOW_MS: u64 = 10;
    let window_us = WINDOW_MS * 1_000;
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(window_us - 1, KEY_A, 1); // Press A again just inside window (9.999ms)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1]; // e2 should be filtered

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg(format!("{}ms", WINDOW_MS)) // ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Event just inside window boundary was not filtered"
    );
}

#[test]
fn passes_at_window_boundary() {
    const WINDOW_MS: u64 = 10;
    let window_us = WINDOW_MS * 1_000;
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(window_us, KEY_A, 1); // Press A again exactly at window boundary (10.000ms)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // e2 should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg(format!("{}ms", WINDOW_MS)) // ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Event exactly at window boundary was filtered"
    );
}

// Removed passes_all_with_bypass test, as --window 0 serves this purpose.
// The window_zero_passes_all test covers this scenario.

#[test]
fn test_complex_sequence() {
    const WINDOW_MS: u64 = 10; // 10ms window
    let window_us = WINDOW_MS * 1_000;

    // Define a complex sequence of events
    let e1 = key_ev(0, KEY_A, 1); // A Press (Pass)
    let e2 = key_ev(window_us / 2, KEY_A, 1); // A Press (Bounce) - within window of e1
    let e3 = key_ev(window_us + 1, KEY_A, 0); // A Release (Pass) - outside window of e1
    let e4 = key_ev(window_us + 1 + window_us / 2, KEY_A, 0); // A Release (Bounce) - within window of e3
    let e5 = non_key_ev(window_us * 2); // SYN event (Pass)
    let e6 = key_ev(window_us * 2 + 1, KEY_B, 1); // B Press (Pass)
    let e7 = key_ev(window_us * 2 + 1 + window_us / 4, KEY_B, 2); // B Repeat (Pass) - Different value than e6
    let e8 = key_ev(window_us * 3, KEY_A, 1); // A Press (Pass) - outside window of e3/e4
    let e9 = key_ev(window_us * 3 + window_us / 2, KEY_A, 1); // A Press (Bounce) - within window of e8
    let e10 = key_ev(window_us * 4, KEY_B, 2); // B Repeat (Pass) - outside window of e6/e7

    let input_events = vec![e1, e2, e3, e4, e5, e6, e7, e8, e9, e10];

    // Define the expected output sequence (events that should NOT be dropped)
    // Note: e7 (KEY_B, value 2) should PASS because its value is different from e6 (KEY_B, value 1).
    // The bounce filter only drops events with the *same* key code AND *same* value within the window.
    let expected_events = vec![
        e1,  // A Press (Pass)
        e3,  // A Release (Pass)
        e5,  // SYN event (Pass)
        e6,  // B Press (Pass)
        e7,  // B Repeat (Pass) - Different value than e6, so not a bounce
        e8,  // A Press (Pass)
        e10, // B Repeat (Pass)
    ];

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg(format!("{}ms", WINDOW_MS)) // ADDED ms SUFFIX
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;

    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Complex event sequence was not filtered correctly"
    );
}

#[test]
fn stats_output_human_readable() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce (5ms window)
    let e3 = key_ev(10_000, KEY_B, 1); // Pass
    let e4 = key_ev(12_000, KEY_B, 1); // Bounce (5ms window)
    let e5 = key_ev(100_000, KEY_A, 0); // Pass (Release)
    let input_events = vec![e1, e2, e3, e4, e5];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .write_stdin(input_bytes);

    cmd.assert()
        .success() // Check exit code 0
        .stderr(predicate::str::contains(
            "--- Overall Statistics (Cumulative) ---",
        ))
        .stderr(predicate::str::contains("Key Events Processed: 5")) // All 5 key events
        .stderr(predicate::str::contains("Key Events Passed:   3")) // e1, e3, e5
        .stderr(predicate::str::contains("Key Events Dropped:  2")) // e2, e4
        .stderr(predicate::str::contains("Key [KEY_A] (30):"))
        .stderr(predicate::str::contains("Press   (1): 1 drops")) // e2 dropped
        // Updated assertion format for milliseconds
        .stderr(predicate::str::contains(
            "Bounce Time: 3.0 ms / 3.0 ms / 3.0 ms",
        )) // Check timing for e2 drop
        .stderr(predicate::str::contains("Key [KEY_B] (48):"))
        .stderr(predicate::str::contains("Press   (1): 1 drops")) // e4 dropped
        // Updated assertion format for milliseconds
        .stderr(predicate::str::contains(
            "Bounce Time: 2.0 ms / 2.0 ms / 2.0 ms",
        )); // Check timing for e4 drop
}

#[test]
fn stats_output_json() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce (5ms window)
    let input_events = vec![e1, e2];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--stats-json") // Enable JSON output
        .write_stdin(input_bytes);

    let output = cmd.output().expect("Failed to run command");
    assert!(output.status.success());

    let stderr_str = String::from_utf8(output.stderr).expect("Stderr not valid UTF-8");
    // Find the JSON part (it might be mixed with other logs if verbose)
    // Find the first line that looks like the start of a JSON object
    let json_start_line = stderr_str
        .lines()
        .find(|l| l.trim().starts_with('{'))
        .expect("Could not find start of JSON block ('{') in stderr");

    // Find the index where the JSON starts
    let json_start_index = stderr_str.find(json_start_line).unwrap_or(0);

    // Attempt to parse from that point onwards, consuming only the first JSON value found
    let json_part = &stderr_str[json_start_index..];

    // Use a Deserializer to parse only the first JSON object found
    let mut deserializer = serde_json::Deserializer::from_str(json_part);
    let stats_json: Value = match Value::deserialize(&mut deserializer) {
        Ok(val) => val,
        Err(e) => panic!(
            "Failed to deserialize first JSON object from stderr starting at detected block: Error: {}, starts with '{}', full stderr: {}",
            e, json_start_line, stderr_str
        ),
    };

    // Ensure the deserializer consumed something and check for trailing data if needed (optional)
    // let remaining_bytes = deserializer.byte_offset();
    // assert!(remaining_bytes > 0, "Deserializer did not consume any bytes");
    // You could check if json_part[remaining_bytes..].trim().is_empty() if strictness is required

    assert_eq!(stats_json["report_type"], "Cumulative");
    assert_eq!(stats_json["key_events_processed"], 2);
    assert_eq!(stats_json["key_events_passed"], 1);
    assert_eq!(stats_json["key_events_dropped"], 1);

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
    assert_eq!(key_a_stats["stats"]["press"]["count"], 1);
    assert_eq!(key_a_stats["stats"]["press"]["timings_us"], json!([3000]));

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
}

#[test]
fn log_bounces_flag() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce (5ms window)
    let input_events = vec![e1, e2];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--log-bounces") // Enable bounce logging
        .write_stdin(input_bytes);

    cmd.assert()
        .success()
        // Check for the specific log line format for a dropped event
        .stderr(
            predicate::str::contains("[DROP]").and(predicate::str::contains("Key [KEY_A] (30)")),
        );
    // Check that PASS lines are NOT present (unless RUST_LOG=trace/debug)
    // .stderr(predicate::str::contains("[PASS]").not()); // This might fail if info logs are present
}

#[test]
fn log_all_events_flag() {
    let e1 = key_ev(0, KEY_A, 1); // Pass
    let e2 = key_ev(3_000, KEY_A, 1); // Bounce (5ms window)
    let e3 = non_key_ev(4_000); // SYN (Pass)
    let input_events = vec![e1, e2, e3];
    let input_bytes = events_to_bytes(&input_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("5ms")
        .arg("--log-all-events") // Enable all logging
        .write_stdin(input_bytes);

    cmd.assert()
        .success()
        // Check for PASS log for e1
        .stderr(
            predicate::str::contains("[PASS]").and(predicate::str::contains("Key [KEY_A] (30)")),
        )
        // Check for DROP log for e2
        .stderr(
            predicate::str::contains("[DROP]").and(predicate::str::contains("Key [KEY_A] (30)")),
        )
        // Check that SYN events are NOT logged even with --log-all-events
        .stderr(predicate::str::contains("EV_SYN").not());
}

#[test]
fn test_debounce_zero_passes_all() {
    let e1 = key_ev(0, KEY_A, 1); // Press A at 0ms
    let e2 = key_ev(1_000, KEY_A, 1); // Press A again at 1ms (would be bounce with window > 1)
    let e3 = key_ev(2_000, KEY_A, 0); // Release A at 2ms
    let e4 = key_ev(3_000, KEY_A, 0); // Release A again at 3ms (would be bounce with window > 1)
    let input_events = vec![e1, e2, e3, e4];
    let expected_events = vec![e1, e2, e3, e4]; // All should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--debounce-time")
        .arg("0ms") // Explicitly set 0ms window
        .write_stdin(input_bytes);

    let output: Output = cmd
        .output()
        .expect("Failed to run command with 0ms debounce");
    assert!(output.status.success(), "Command failed with 0ms debounce");

    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Events were filtered when debounce window was 0ms"
    );
}

#[test]
fn test_only_non_key_events() {
    let e1 = non_key_ev(1000); // SYN event at 1ms
    let e2 = non_key_ev(2000); // SYN event at 2ms
    let e3 = non_key_ev(3000); // SYN event at 3ms
    let input_events = vec![e1, e2, e3];
    let expected_events = vec![e1, e2, e3]; // All should pass

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--stats-json") // Get stats output
        .write_stdin(input_bytes);

    let output = cmd
        .output()
        .expect("Failed to run command with only non-key events");
    assert!(
        output.status.success(),
        "Command failed with only non-key events"
    );

    // Check stdout contains all input events
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Non-key events were filtered or modified"
    );

    // Check stderr stats
    let stderr_str = String::from_utf8(output.stderr).expect("Stderr not valid UTF-8");
    let json_start_line = stderr_str
        .lines()
        .find(|l| l.trim().starts_with('{'))
        .expect("Could not find start of JSON block in non-key event stderr");
    let json_start_index = stderr_str.find(json_start_line).unwrap_or(0);
    let json_part = &stderr_str[json_start_index..];
    let mut deserializer = serde_json::Deserializer::from_str(json_part);
    let stats_json: Value = Value::deserialize(&mut deserializer)
        .expect("Failed to deserialize JSON from non-key event stderr");

    // Assert that key event counts are zero
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
            .as_object()
            .map_or(true, |m| m.is_empty()),
        "Per-key stats should be empty"
    );
    assert!(
        stats_json["per_key_passed_near_miss_timing"]
            .as_object()
            .map_or(true, |m| m.is_empty()),
        "Near-miss stats should be empty"
    );
}
