#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use input_linux_sys::{input_event, timeval};
use intercept_bounce::config::Config;
use intercept_bounce::filter::stats::StatsCollector;
use intercept_bounce::logger::EventInfo;
use libfuzzer_sys::fuzz_target;
use std::time::Duration;

// Helper struct that CAN derive Arbitrary
#[derive(Arbitrary, Debug, Clone)]
struct ArbitraryEventData {
    // EventInfo fields
    event_type: u16,  // Keep it simple, maybe focus on EV_KEY?
    event_code: u16,  // Limit range? e.g., 0..1024
    event_value: i32, // Limit range? e.g., 0..3
    event_us: u64,
    is_bounce: bool,
    diff_us_present: bool, // Control if diff_us is Some or None
    diff_us_value: u64,
    last_passed_us_present: bool, // Control if last_passed_us is Some or None
    last_passed_us_value: u64,

    // Config fields
    debounce_ms: u64, // Use ms to avoid huge durations
    near_miss_ms: u64,
    // Other config bools if needed
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    if let Ok(arb_data) = ArbitraryEventData::arbitrary(&mut u) {
        // Construct Config
        let config = Config::new(
            Duration::from_millis(arb_data.debounce_ms),
            Duration::from_millis(arb_data.near_miss_ms),
            Duration::ZERO, // log_interval not relevant here
            false,
            false,
            false,
            false, // other flags not relevant
            "info".to_string(),
        );

        // Construct EventInfo
        let diff_us = if arb_data.diff_us_present {
            Some(arb_data.diff_us_value)
        } else {
            None
        };
        let last_passed_us = if arb_data.last_passed_us_present {
            Some(arb_data.last_passed_us_value)
        } else {
            None
        };

        // Ensure event_us >= last_passed_us if present, to avoid trivial checked_sub(None)
        // This makes the fuzzer focus on valid time sequences for diff calculation.
        let valid_last_passed_us = last_passed_us.filter(|&last| arb_data.event_us >= last);
        // Ensure diff_us makes sense relative to timestamps if both present
        let valid_diff_us = diff_us.filter(|&diff| {
            if let Some(last) = valid_last_passed_us {
                arb_data.event_us.checked_sub(last) == Some(diff)
            } else {
                // If no last_passed, diff_us should ideally be None for a real bounce.
                // But the fuzzer might generate Some(diff) anyway.
                // We allow it here to test the stats recording logic with potentially inconsistent data.
                true
            }
        });

        let event_info = EventInfo {
            event: input_event {
                // Construct a plausible timeval from event_us
                time: timeval {
                    tv_sec: (arb_data.event_us / 1_000_000) as i64,
                    tv_usec: (arb_data.event_us % 1_000_000) as i64,
                },
                // Use arbitrary type, code, value - maybe restrict ranges later
                type_: arb_data.event_type,
                code: arb_data.event_code,
                value: arb_data.event_value,
            },
            event_us: arb_data.event_us,
            is_bounce: arb_data.is_bounce,
            diff_us: valid_diff_us,
            last_passed_us: valid_last_passed_us,
        };

        // Create StatsCollector and call the target function
        let mut stats = StatsCollector::with_capacity();
        stats.record_event_info_with_config(&event_info, &config);

        // Optional: Call printing functions too, to fuzz serialization/formatting
        // let mut sink = std::io::sink();
        // stats.print_stats_json(&config, Some(123), "Fuzz", &mut sink);
        // stats.print_stats_to_stderr(&config, "Fuzz");
    }
});
