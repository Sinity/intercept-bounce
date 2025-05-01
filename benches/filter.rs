use criterion::{criterion_group, criterion_main, Criterion};
use intercept_bounce::filter::BounceFilter;
use intercept_bounce::logger::{Logger, EventInfo, LogMessage};
use input_linux_sys::{input_event, timeval, EV_KEY, EV_SYN};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use crossbeam_channel::bounded;


// Helper to create an input_event
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

// Helper to create EventInfo
fn create_event_info(ts_us: u64, code: u16, value: i32, is_bounce: bool, diff_us: Option<u64>, last_passed_us: Option<u64>) -> EventInfo {
    EventInfo {
        event: key_ev(ts_us, code, value),
        event_us: ts_us,
        is_bounce,
        diff_us,
        last_passed_us,
    }
}

// Helper to create a non-key EventInfo (SYN)
fn create_syn_info(ts_us: u64) -> EventInfo {
     EventInfo {
        event: input_event {
            time: timeval {
                tv_sec: (ts_us / 1_000_000) as i64,
                tv_usec: (ts_us % 1_000_000) as i64,
            },
            type_: EV_SYN as u16,
            code: 0, // SYN_REPORT
            value: 0,
        },
        event_us: ts_us,
        is_bounce: false, // SYN events are never bounces
        diff_us: None,
        last_passed_us: None,
    }
}


fn bench_filter_check_event(c: &mut Criterion) {
    let debounce_us = 10_000; // 10ms debounce

    // Benchmark a passing scenario (first event or outside window)
    c.bench_function("filter::check_event_pass", |b| {
        let event_iter = key_ev(0, 30, 1); // First event
        b.iter(|| {
            // Reset filter state for each iteration to simulate independent events
            let mut filter_reset = BounceFilter::new();
            filter_reset.check_event(&event_iter, debounce_us);
        })
    });

    // Benchmark a bounce scenario
    c.bench_function("filter::check_event_bounce", |b| {
        let mut filter_iter = BounceFilter::new();
        let event_pass = key_ev(0, 30, 1);
        filter_iter.check_event(&event_pass, debounce_us); // Pass the first event
        let event_bounce = key_ev(debounce_us / 2, 30, 1); // Bounce event

        b.iter(|| {
             // Reset filter state and pass the first event before checking the bounce
             let mut filter_reset = BounceFilter::new();
             let event_pass_iter = key_ev(0, 30, 1);
             filter_reset.check_event(&event_pass_iter, debounce_us); // Pass
             let event_bounce_iter = key_ev(debounce_us / 2, 30, 1); // Bounce
             filter_reset.check_event(&event_bounce_iter, debounce_us); // Check bounce
        })
    });

    // Benchmark a non-key event scenario
     c.bench_function("filter::check_event_non_key", |b| {
        let mut filter_iter = BounceFilter::new();
        let event_non_key = input_event { time: timeval { tv_sec: 0, tv_usec: 0 }, type_: EV_SYN as u16, code: 0, value: 0 };

        b.iter(|| {
             let mut filter_reset = BounceFilter::new();
             filter_reset.check_event(&event_non_key, debounce_us);
        })
    });
}

fn bench_logger_process_message(c: &mut Criterion) {
    // Setup dummy logger components
    let (_sender, receiver) = bounded::<LogMessage>(1); // Channel not used in process_message directly
    let running = Arc::new(AtomicBool::new(true));
    let debounce_us = 10_000;

    // Create sample EventInfo messages
    let passed_info = create_event_info(10_000, 30, 1, false, None, Some(0)); // Passed event
    let bounced_info = create_event_info(15_000, 30, 1, true, Some(5_000), Some(10_000)); // Bounced event
    let near_miss_info = create_event_info(25_000, 30, 1, false, None, Some(10_000)); // Near miss passed event (diff 15000)
    let syn_info = create_syn_info(30_000); // SYN event

    // Benchmark processing messages with different logging configurations
    c.bench_function("logger::process_message_passed_no_log", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), false, false, 0, false, debounce_us, false); // No logging, not verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(passed_info.clone()));
        })
    });

     c.bench_function("logger::process_message_bounced_no_log", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), false, false, 0, false, debounce_us, false); // No logging, not verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(bounced_info.clone()));
        })
    });

    c.bench_function("logger::process_message_passed_log_all", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), true, false, 0, false, debounce_us, false); // Log all, not verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(passed_info.clone()));
        })
    });

     c.bench_function("logger::process_message_bounced_log_bounces", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), false, true, 0, false, debounce_us, false); // Log bounces, not verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(bounced_info.clone()));
        })
    });

     c.bench_function("logger::process_message_bounced_log_all", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), true, false, 0, false, debounce_us, false); // Log all, not verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(bounced_info.clone()));
        })
    });

     c.bench_function("logger::process_message_near_miss_log_all", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), true, false, 0, false, debounce_us, false); // Log all, not verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(near_miss_info.clone()));
        })
    });

    c.bench_function("logger::process_message_syn_log_all", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), true, false, 0, false, debounce_us, false); // Log all, not verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(syn_info.clone())); // SYN events should be skipped
        })
    });

    // Add benchmarks with verbose logging enabled
    c.bench_function("logger::process_message_passed_log_all_verbose", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), true, false, 0, false, debounce_us, true); // Log all, verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(passed_info.clone()));
        })
    });

     c.bench_function("logger::process_message_bounced_log_all_verbose", |b| {
        let mut logger = Logger::new(receiver.clone(), running.clone(), true, false, 0, false, debounce_us, true); // Log all, verbose
        b.iter(|| {
            logger.process_message(LogMessage::Event(bounced_info.clone()));
        })
    });
}


criterion_group!(benches, bench_filter_check_event, bench_logger_process_message);
criterion_main!(benches);
