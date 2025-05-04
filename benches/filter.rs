use criterion::{criterion_group, criterion_main, Criterion};
use input_linux_sys::{input_event, timeval, EV_KEY, EV_SYN};
use intercept_bounce::config::Config;
use intercept_bounce::filter::stats::StatsCollector; // Import StatsCollector
use intercept_bounce::filter::BounceFilter; // Add this import
use intercept_bounce::logger::{EventInfo, LogMessage, Logger};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread; // Add thread import
use std::time::Duration;

// Import crossbeam-channel directly
use crossbeam_channel::{bounded, Receiver, Sender};

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
fn create_event_info(
    ts_us: u64,
    code: u16,
    value: i32,
    is_bounce: bool,
    diff_us: Option<u64>,
    last_passed_us: Option<u64>,
) -> EventInfo {
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
    let debounce_time = Duration::from_millis(10); // 10ms debounce

    // Pre-create events for reuse in the closures
    let event_pass = key_ev(0, 30, 1); // First event
    let event_bounce = key_ev(debounce_time.as_micros() as u64 / 2, 30, 1); // Bounce event
    let event_non_key = input_event {
        time: timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        type_: EV_SYN as u16,
        code: 0,
        value: 0,
    };

    // Benchmark a passing scenario (first event or outside window)
    c.bench_function("filter::check_event_pass", |b| {
        b.iter(|| {
            let mut filter = BounceFilter::new();
            filter.check_event(&event_pass, debounce_time); // Pass Duration
        })
    });

    // Benchmark a bounce scenario
    c.bench_function("filter::check_event_bounce", |b| {
        b.iter(|| {
            let mut filter = BounceFilter::new();
            filter.check_event(&event_pass, debounce_time); // Pass Duration
            filter.check_event(&event_bounce, debounce_time); // Check bounce with Duration
        })
    });

    // Benchmark a non-key event scenario
    c.bench_function("filter::check_event_non_key", |b| {
        b.iter(|| {
            let mut filter = BounceFilter::new();
            filter.check_event(&event_non_key, debounce_time); // Pass Duration
        })
    });
}

// Helper to create a dummy Config Arc for benches
fn dummy_config(
    debounce_time: Duration,
    near_miss_threshold: Duration,
    log_all: bool,
    log_bounces: bool,
    log_interval: Duration,
    stats_json: bool,
    verbose: bool,
) -> Arc<Config> {
    Arc::new(Config::new(
        // Use the new constructor
        debounce_time,
        near_miss_threshold,
        log_interval,
        log_all, // log_all_events
        log_bounces,
        stats_json,
        verbose,
        "info".to_string(), // log_filter
    ))
}

// Helper to create a populated StatsCollector (example)
fn create_populated_stats() -> StatsCollector {
    let mut stats = StatsCollector::with_capacity();
    let config = dummy_config(
        Duration::from_millis(10),
        Duration::from_millis(100),
        false,
        false,
        Duration::ZERO,
        false,
        false,
    );
    // Add some events using stats.record_event_info_with_config(...)
    // Example: Add a passed event, a bounced event, a near-miss event for KEY_A=30
    let ev1 = create_event_info(0, 30, 1, false, None, None);
    let ev2 = create_event_info(5_000, 30, 1, true, Some(5_000), Some(0));
    let ev3 = create_event_info(15_000, 30, 1, false, None, Some(0)); // Near miss relative to ev1
    let ev4 = create_event_info(20_000, 48, 1, false, None, None); // KEY_B
    stats.record_event_info_with_config(&ev1, &config);
    stats.record_event_info_with_config(&ev2, &config);
    stats.record_event_info_with_config(&ev3, &config);
    stats.record_event_info_with_config(&ev4, &config);
    stats
}

fn bench_logger_process_message(c: &mut Criterion) {
    // Setup dummy logger components
    // Conditionally create the appropriate receiver type for the benchmark
    let (_sender, receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(1);

    let running = Arc::new(AtomicBool::new(true));
    let debounce_time = Duration::from_millis(10); // 10ms
    let near_miss_threshold = Duration::from_millis(100); // 100ms
    let log_interval = Duration::ZERO;

    // Create sample EventInfo messages
    let passed_info = create_event_info(
        debounce_time.as_micros() as u64,
        30,
        1,
        false,
        None,
        Some(0),
    ); // Passed event
    let bounced_info = create_event_info(15_000, 30, 1, true, Some(5_000), Some(10_000)); // Bounced event (adjust ts if needed)
    let near_miss_info = create_event_info(25_000, 30, 1, false, None, Some(10_000)); // Near miss passed event (adjust ts if needed)
    let syn_info = create_syn_info(30_000); // SYN event

    // Benchmark processing messages with different logging configurations
    c.bench_function("logger::process_message_passed_no_log", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            false,
            false,
            log_interval,
            false,
            false,
        ); // No logging, not verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_event_info(
                debounce_time.as_micros() as u64, 30, 1, false, None, Some(0)
            ));
            logger.process_message(msg);
        })
    });

    c.bench_function("logger::process_message_bounced_no_log", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            false,
            false,
            log_interval,
            false,
            false,
        ); // No logging, not verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_event_info(
                15_000, 30, 1, true, Some(5_000), Some(10_000)
            ));
            logger.process_message(msg);
        })
    });

    c.bench_function("logger::process_message_passed_log_all", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            true,
            false,
            log_interval,
            false,
            false,
        ); // Log all, not verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
             let msg = LogMessage::Event(create_event_info(
                debounce_time.as_micros() as u64, 30, 1, false, None, Some(0)
            ));
            logger.process_message(msg);
        })
    });

    c.bench_function("logger::process_message_bounced_log_bounces", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            false,
            true,
            log_interval,
            false,
            false,
        ); // Log bounces, not verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_event_info(
                15_000, 30, 1, true, Some(5_000), Some(10_000)
            ));
            logger.process_message(msg);
        })
    });

    c.bench_function("logger::process_message_bounced_log_all", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            true,
            false,
            log_interval,
            false,
            false,
        ); // Log all, not verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_event_info(
                15_000, 30, 1, true, Some(5_000), Some(10_000)
            ));
            logger.process_message(msg);
        })
    });

    c.bench_function("logger::process_message_near_miss_log_all", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            true,
            false,
            log_interval,
            false,
            false,
        ); // Log all, not verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_event_info(
                25_000, 30, 1, false, None, Some(10_000)
            ));
            logger.process_message(msg);
        })
    });

    c.bench_function("logger::process_message_syn_log_all", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            true,
            false,
            log_interval,
            false,
            false,
        ); // Log all, not verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_syn_info(30_000));
            logger.process_message(msg); // SYN events should be skipped
        })
    });

    // Add benchmarks with verbose logging enabled
    c.bench_function("logger::process_message_passed_log_all_verbose", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            true,
            false,
            log_interval,
            false,
            true,
        ); // Log all, verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_event_info(
                debounce_time.as_micros() as u64, 30, 1, false, None, Some(0)
            ));
            logger.process_message(msg);
        })
    });

    c.bench_function("logger::process_message_bounced_log_all_verbose", |b| {
        let cfg = dummy_config(
            debounce_time,
            near_miss_threshold,
            true,
            false,
            log_interval,
            false,
            true,
        ); // Log all, verbose
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(create_event_info(
                15_000, 30, 1, true, Some(5_000), Some(10_000)
            ));
            logger.process_message(msg);
        })
    });
}

fn bench_stats_collector_record(c: &mut Criterion) {
    let debounce_time = Duration::from_millis(10);
    let near_miss_threshold = Duration::from_millis(100);
    let config_base = dummy_config(
        debounce_time,
        near_miss_threshold,
        false,
        false,
        Duration::ZERO,
        false,
        false,
    );
    let config_near_miss_short = dummy_config(
        debounce_time,
        Duration::from_millis(20),
        false,
        false,
        Duration::ZERO,
        false,
        false,
    );

    let passed_info = create_event_info(20_000, 30, 1, false, None, Some(0));
    let bounced_info = create_event_info(5_000, 30, 1, true, Some(5_000), Some(0));
    let near_miss_info = create_event_info(15_000, 30, 1, false, None, Some(0)); // Near miss for 100ms threshold
    let syn_info = create_syn_info(25_000);

    c.bench_function("stats::record_passed", |b| {
        let mut stats = StatsCollector::with_capacity();
        b.iter(|| stats.record_event_info_with_config(&passed_info, &config_base))
    });
    c.bench_function("stats::record_bounced", |b| {
        let mut stats = StatsCollector::with_capacity();
        b.iter(|| stats.record_event_info_with_config(&bounced_info, &config_base))
    });
    c.bench_function("stats::record_near_miss", |b| {
        let mut stats = StatsCollector::with_capacity();
        b.iter(|| stats.record_event_info_with_config(&near_miss_info, &config_base))
    });
    c.bench_function("stats::record_near_miss_short_thresh", |b| {
        let mut stats = StatsCollector::with_capacity();
        // This should *not* record as near miss with the short threshold config
        b.iter(|| stats.record_event_info_with_config(&near_miss_info, &config_near_miss_short))
    });
    c.bench_function("stats::record_syn", |b| {
        let mut stats = StatsCollector::with_capacity();
        b.iter(|| stats.record_event_info_with_config(&syn_info, &config_base))
    });
}

fn bench_stats_collector_print(c: &mut Criterion) {
    let stats = create_populated_stats();
    let config = dummy_config(
        Duration::from_millis(10),
        Duration::from_millis(100),
        false,
        false,
        Duration::from_secs(900),
        false,
        false,
    );
    let runtime = Some(123_456_789); // Example runtime

    c.bench_function("stats::print_json", |b| {
        b.iter(|| {
            let mut writer = Vec::new(); // Write to buffer
            stats.print_stats_json(&config, runtime, "Benchmark", &mut writer);
            criterion::black_box(writer); // Prevent optimization
        })
    });

    // Benchmark human-readable formatting, writing to sink() to discard output
    c.bench_function("stats::print_human", |b| {
        b.iter(|| {
            let mut writer = std::io::sink(); // Discard output
                                              // Call the new formatting function directly, passing the sink writer
            stats
                .format_stats_human_readable(&config, "Benchmark", &mut writer)
                .expect("Formatting human-readable stats failed"); // Handle potential error
            criterion::black_box(writer); // Prevent optimization
        })
    });
}

fn bench_logger_channel_send(c: &mut Criterion) {
    const BURST_SIZE: usize = 100;
    const QUEUE_CAPACITY: usize = 1024;

    // Create a dummy message to send
    let dummy_event_info = create_event_info(1000, 30, 1, false, None, None);
    // let message = LogMessage::Event(dummy_event_info); // This is unused now

    // Benchmark only crossbeam-channel
    let (sender, receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(QUEUE_CAPACITY);
    let dummy_logger_handle = thread::spawn(move || {
        while let Ok(_) = receiver.recv() {
            thread::yield_now();
        }
    });

    c.bench_function("logger::channel_send_burst", |b| {
        // Remove feature name from bench name
        b.iter_batched(
            || sender.clone(),
            |s| {
                let mut success_count = 0;
                let mut drop_count = 0;
                for _ in 0..BURST_SIZE {
                    // Recreate the message inside the loop as try_send takes ownership
                    // Note: dummy_event_info itself doesn't need cloning here as it's Copy
                    let msg_to_send = LogMessage::Event(dummy_event_info);
                    match s.try_send(msg_to_send) {
                        Ok(_) => success_count += 1,
                        Err(_) => drop_count += 1,
                    }
                }
                (success_count, drop_count)
            },
            criterion::BatchSize::SmallInput,
        )
    });
    drop(sender);
    dummy_logger_handle
        .join()
        .expect("Dummy logger thread panicked");
}

criterion_group!(
    benches,
    bench_filter_check_event,
    bench_logger_process_message,
    bench_stats_collector_record,
    bench_stats_collector_print,
    bench_logger_channel_send // Add the channel benchmark
);
criterion_main!(benches);
