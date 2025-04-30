use assert_cmd::Command;
// Removed: use assert_cmd::output::OutputOkExt; // Import the trait that provides .unwrap() on the output Result
use input_linux_sys::{input_event, timeval, EV_KEY};
use std::io::Write;
use std::mem::size_of;
use std::process::Output; // Import Output struct

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
    cmd.arg("--window")
        .arg("5") // 5ms window
        .write_stdin(input_bytes);

    // Execute the command and get the owned Output struct
    let output: Output = cmd.output().unwrap();

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
    cmd.arg("--window")
        .arg("5") // 5ms window
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
    cmd.arg("--window")
        .arg("5") // 5ms window
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
    cmd.arg("--window")
        .arg("5") // 5ms window
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
    cmd.arg("--window")
        .arg("5") // 5ms window
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
    // Note: Key repeats (value 2) often follow a press (value 1) after a delay.
    // Bouncing usually happens on the initial press or release edge.
    // However, if a repeat signal itself bounces (e.g., faulty hardware sends two repeats too close),
    // we should filter the second one.
    let e1 = key_ev(0, KEY_A, 1);     // Press A at 0ms
    let e2 = key_ev(500_000, KEY_A, 2); // Repeat A at 500ms (normal repeat)
    let e3 = key_ev(502_000, KEY_A, 2); // Repeat A again at 502ms (bounce of e2)
    let input_events = vec![e1, e2, e3];
    let expected_events = vec![e1, e2]; // Bounce e3 dropped

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--window")
        .arg("5") // 5ms window
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Key repeat bounce was not filtered"
    );
}

#[test]
fn window_zero_passes_all() {
    let e1 = key_ev(0, KEY_A, 1);     // Press A at 0ms
    let e2 = key_ev(1_000, KEY_A, 1); // Press A again at 1ms (would be bounce with window > 1)
    let input_events = vec![e1, e2];
    let expected_events = vec![e1, e2]; // Both should pass when window is 0

    let input_bytes = events_to_bytes(&input_events);
    let expected_output_bytes = events_to_bytes(&expected_events);

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--window")
        .arg("0") // 0ms window
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
    cmd.arg("--window")
        .arg("5") // 5ms window
        .write_stdin(input_bytes);

    let output: Output = cmd.output().unwrap();
    let actual_stdout_bytes = output.stdout;
    assert_eq!(
        actual_stdout_bytes, expected_output_bytes,
        "Event with earlier timestamp was dropped"
    );
}
