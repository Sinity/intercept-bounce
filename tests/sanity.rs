use assert_cmd::Command;
use linux_input_sys::{input_event, timeval};
use std::io::Write;
use std::mem::size_of;

fn fake_ev(ts: u64) -> input_event {
    input_event {
        time:  timeval { tv_sec: (ts / 1_000_000) as i64,
                         tv_usec: (ts % 1_000_000) as i64 },
        type_: libc::EV_KEY,
        code: 30,          // KEY_A
        value: 1,          // press
    }
}

#[test]
fn drops_bounce() {
    let mut input: Vec<u8> = Vec::new();
    let e1 = fake_ev(0);
    let e2 = fake_ev(3_000);    // 3 ms later, should be dropped
    unsafe {
        input.write_all(std::slice::from_raw_parts(
            &e1 as *const _ as *const u8,
            size_of::<input_event>(),
        )).unwrap();
        input.write_all(std::slice::from_raw_parts(
            &e2 as *const _ as *const u8,
            size_of::<input_event>(),
        )).unwrap();
    }

   let mut cmd = Command::cargo_bin("intercept-bounce").unwrap();
    cmd.arg("--window").arg("5")
        .write_stdin(input)
        .assert()
        .stdout(predicates::str::is_empty()); // Expect empty stdout if bounce is dropped
}
