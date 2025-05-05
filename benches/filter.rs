use criterion::{black_box, criterion_group, criterion_main, Criterion};
use input_linux_sys::{input_event, EV_KEY, EV_SYN};
use intercept_bounce::config::Config;
use intercept_bounce::filter::stats::StatsCollector;
use intercept_bounce::filter::BounceFilter;
use intercept_bounce::logger::{EventInfo, LogMessage, Logger};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};

mod common; // Include the common module
use common::*; // Import helpers

fn bench_filter_check_event(c: &mut Criterion) {
    let debounce_time = Duration::from_millis(10); // 10ms debounce

    // Pre-create events for reuse in the closures
    let event_pass = key_ev(0, 30, 1); // First event
    let event_bounce = key_ev(debounce_time.as_micros() as u64 / 2, 30, 1); // Bounce event
    let event_non_key = non_key_ev(0);

    // Benchmark a passing scenario (first event or outside window)
    c.bench_function("filter::check_event_pass", |b| {
        b.iter(|| {
            let mut filter = BounceFilter::new();
            // Call check_event and use black_box to prevent optimizing away the call
            black_box(filter.check_event(&event_pass, debounce_time));
        })
    });

    // Benchmark a bounce scenario
    c.bench_function("filter::check_event_bounce", |b| {
        b.iter(|| {
            let mut filter = BounceFilter::new();
            black_box(filter.check_event(&event_pass, debounce_time));
            black_box(filter.check_event(&event_bounce, debounce_time));
        })
    });

    // Benchmark a non-key event scenario
    c.bench_function("filter::check_event_non_key", |b| {
        b.iter(|| {
            let mut filter = BounceFilter::new();
            black_box(filter.check_event(&event_non_key, debounce_time));
        })
    });
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
    // Example: Add a passed event, a bounced event, a near-miss event for KEY_A
    let ev1 = passed_event_info(key_ev(0, KEY_A, 1), 0, None);
    let ev2 = bounced_event_info(key_ev(5_000, KEY_A, 1), 5_000, 5_000, Some(0));
    let ev3 = passed_event_info(key_ev(15_000, KEY_A, 1), 15_000, Some(0)); // Near miss relative to ev1
    let ev4 = create_event_info(20_000, 48, 1, false, None, None); // KEY_B
    stats.record_event_info_with_config(&ev1, &config);
    stats.record_event_info_with_config(&ev2, &config);
    stats.record_event_info_with_config(&ev3, &config);
    stats.record_event_info_with_config(&ev4, &config);
    stats
}

fn bench_logger_process_message(c: &mut Criterion) {
    // Setup dummy logger components
    let (_sender, receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(1);

    let running = Arc::new(AtomicBool::new(true));
    let debounce_time = Duration::from_millis(10); // 10ms
    let near_miss_threshold = Duration::from_millis(100); // 100ms
    let log_interval = Duration::ZERO;

    // These are no longer needed here as they are created inside the closures below
    // let passed_info = ...
    // let bounced_info = ...
    // let near_miss_info = ...
    // let syn_info = ...

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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(passed_event_info(key_ev(debounce_time.as_micros() as u64, 30, 1), debounce_time.as_micros() as u64, Some(0)));
            logger.process_message(msg, &None);
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(bounced_event_info(key_ev(15_000, 30, 1), 15_000, 5_000, Some(10_000)));
            logger.process_message(msg, &None);
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(passed_event_info(key_ev(debounce_time.as_micros() as u64, 30, 1), debounce_time.as_micros() as u64, Some(0)));
            logger.process_message(msg, &None);
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(bounced_event_info(key_ev(15_000, 30, 1), 15_000, 5_000, Some(10_000)));
            logger.process_message(msg, &None);
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(bounced_event_info(key_ev(15_000, 30, 1), 15_000, 5_000, Some(10_000)));
            logger.process_message(msg, &None);
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(passed_event_info(key_ev(25_000, 30, 1), 25_000, Some(10_000)));
            logger.process_message(msg, &None);
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None);
        // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(passed_event_info(non_key_ev(30_000), 30_000, None)); // SYN events are always passed
            logger.process_message(msg, &None);
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None); // Add None for otel_meter
                                                                                    // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(passed_event_info(key_ev(debounce_time.as_micros() as u64, 30, 1), debounce_time.as_micros() as u64, Some(0)));
            logger.process_message(msg, &None); // Add &None for near_miss_counter
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
        let mut logger = Logger::new(receiver.clone(), running.clone(), cfg, None); // Add None for otel_meter
                                                                                    // Recreate the EventInfo inside the closure for each iteration
        b.iter(|| {
            let msg = LogMessage::Event(bounced_event_info(key_ev(15_000, 30, 1), 15_000, 5_000, Some(10_000)));
            logger.process_message(msg, &None); // Add &None for near_miss_counter
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

    let passed_info = passed_event_info(key_ev(20_000, 30, 1), 20_000, Some(0));
    let bounced_info = bounced_event_info(key_ev(5_000, 30, 1), 5_000, 5_000, Some(0));
    let near_miss_info = passed_event_info(key_ev(15_000, 30, 1), 15_000, Some(0)); // Near miss for 100ms threshold
    let syn_info = passed_event_info(non_key_ev(25_000), 25_000, None); // SYN events are always passed

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

    // Benchmark only crossbeam-channel
    let (sender, receiver): (Sender<LogMessage>, Receiver<LogMessage>) = bounded(QUEUE_CAPACITY);
    let dummy_logger_handle = thread::spawn(move || {
        while receiver.recv().is_ok() {
            thread::yield_now();
        }
    });

    c.bench_function("logger::channel_send_burst", |b| {
        b.iter_batched(
            || sender.clone(),
            |s| {
                let mut success_count = 0;
                let mut drop_count = 0;
                for _ in 0..BURST_SIZE {
                    // Recreate both EventInfo and LogMessage inside the loop
                    let dummy_event_info_inner = passed_event_info(key_ev(1000, 30, 1), 1000, None);
                    let msg_to_send = LogMessage::Event(dummy_event_info_inner);
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
    bench_logger_channel_send
);
criterion_main!(benches);
