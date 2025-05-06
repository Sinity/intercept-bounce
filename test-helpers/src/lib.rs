//! Common helper functions for tests and benchmarks.
use input_linux_sys::{input_event, timeval, EV_KEY, EV_SYN};
use intercept_bounce::{config::Config, logger::EventInfo};
use std::sync::Arc;
use std::time::Duration;

// --- Constants ---
pub const KEY_A: u16 = 30;
pub const KEY_B: u16 = 48;
pub const KEY_C: u16 = 46;
pub const KEY_D: u16 = 32; // Added KEY_D for tests
pub const DEBOUNCE_TIME: Duration = Duration::from_millis(10); // Standard debounce time for tests

// --- Event Creation Helpers ---

/// Creates an EV_KEY input_event with a specific microsecond timestamp.
pub fn key_ev(ts_us: u64, code: u16, value: i32) -> input_event {
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

/// Creates a non-key input_event (e.g., EV_SYN) with a specific microsecond timestamp.
pub fn non_key_ev(ts_us: u64) -> input_event {
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

// --- EventInfo Creation Helpers ---

/// Creates an EventInfo struct simulating a passed event.
pub fn passed_event_info(
    event: input_event,
    event_us: u64,
    last_passed_us: Option<u64>,
) -> EventInfo {
    EventInfo {
        event,
        event_us,
        is_bounce: false,
        diff_us: None,
        last_passed_us,
    }
}

/// Creates an EventInfo struct simulating a bounced (dropped) event.
pub fn bounced_event_info(
    event: input_event,
    event_us: u64,
    diff_us: u64,
    last_passed_us: Option<u64>,
) -> EventInfo {
    EventInfo {
        event,
        event_us,
        is_bounce: true,
        diff_us: Some(diff_us),
        last_passed_us,
    }
}

// --- Config Creation Helper ---

/// Helper to create a dummy Config Arc for tests/benches
pub fn dummy_config(
    debounce_time: Duration,
    near_miss_threshold: Duration,
    log_interval: Duration,
    log_all: bool,
    log_bounces: bool,
    stats_json: bool,
    verbose: bool,
) -> Arc<Config> {
    Arc::new(Config::new(
        debounce_time,
        near_miss_threshold,
        log_interval,
        log_all,
        log_bounces,
        stats_json,
        verbose,
        "info".to_string(),
        None,
        0,
    ))
}

/// Helper to create a dummy Config (non-Arc) for tests
pub fn dummy_config_no_arc(debounce_time: Duration, near_miss_threshold: Duration) -> Config {
    Config::new(
        debounce_time,
        near_miss_threshold,
        Duration::ZERO,     // log_interval (not relevant for these tests)
        false,              // log_all_events (not relevant)
        false,              // log_bounces (not relevant)
        false,              // stats_json (not relevant for accumulation logic)
        false,              // verbose (not relevant)
        "info".to_string(), // log_filter (not relevant)
        None,               // otel_endpoint (not relevant)
        0,
    )
}
