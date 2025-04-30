use assert_cmd::Command;
use input_linux_sys::{input_event, timeval, EV_KEY};
// Removed: use predicates::bytes::eq; // Import eq predicate for bytes
use std::io::Write;
use std::mem::size_of;

fn fake_ev(ts: u64) -> input_event {
    input_event {
        time:  timeval { tv_sec: (ts / 1_000_000) as i64,
                         tv_usec: (ts % 1_000_000) as i64 },
        type_: EV_KEY as u16,
        code: 30,          // KEY_A
        value: 1,          // press
    }
}

#[test]
fn drops_bounce() {
    let mut input: Vec<u8> = Vec::new();
    let e1 = fake_ev(0);
    let e2 = fake_ev(3_000);    // 3 ms later, should be dropped

    // Write e1 to input and capture its bytes for expected output
    unsafe {
        input.write_all(std::slice::from_raw_parts(
            &e1 as *const _ as *const u8,
            size_of::<input_event>(),
        )).unwrap();
    }
    // The expected output is just the first event
    let expected_output_bytes = input.clone();

    // Write e2 to input
    unsafe {
        input.write_all(std::slice::from_raw_parts(
            &e2 as *const _ as *const u8,
            size_of::<input_event>(),
        )).unwrap();
    }

    let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    let assert = cmd.arg("--window").arg("5")
        .write_stdin(input)
        .assert();

    // Capture stdout bytes and use standard assertion
    let actual_stdout_bytes = assert.get_stdout();
    assert_eq!(actual_stdout_bytes, expected_output_bytes);
}
