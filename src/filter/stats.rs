use std::collections::HashMap;
use crate::filter::keynames::get_key_name;

#[derive(Default, Debug)]
pub struct KeyValueStats {
    pub count: u64,
    pub timings_us: Vec<u64>,
}

#[derive(Default, Debug)]
pub struct KeyStats {
    pub press: KeyValueStats,
    pub release: KeyValueStats,
    pub repeat: KeyValueStats,
}

pub struct StatsCollector {
    pub key_events_processed: u64,
    pub key_events_passed: u64,
    pub key_events_dropped: u64,
    pub per_key_stats: HashMap<u16, KeyStats>,
    pub per_key_passed_near_miss_timing: HashMap<(u16, i32), Vec<u64>>,
}

impl StatsCollector {
    pub fn new() -> Self {
        StatsCollector {
            key_events_processed: 0,
            key_events_passed: 0,
            key_events_dropped: 0,
            per_key_stats: HashMap::with_capacity(1024),
            per_key_passed_near_miss_timing: HashMap::with_capacity(1024),
        }
    }

    pub fn record_event(&mut self, key_code: u16, key_value: i32, is_bounce: bool, bounce_diff_us: Option<u64>) {
        self.key_events_processed += 1;
        if is_bounce {
            self.key_events_dropped += 1;
            let key_stats = self.per_key_stats.entry(key_code).or_default();
            let value_stats = match key_value {
                1 => &mut key_stats.press,
                0 => &mut key_stats.release,
                _ => &mut key_stats.repeat,
            };
            value_stats.count += 1;
            if let Some(diff) = bounce_diff_us {
                value_stats.timings_us.push(diff);
            }
        } else {
            self.key_events_passed += 1;
        }
    }

    pub fn record_near_miss(&mut self, key: (u16, i32), diff: u64) {
        self.per_key_passed_near_miss_timing.entry(key).or_default().push(diff);
    }

    pub fn print_stats(
        &self,
        debounce_time_us: u64,
        log_all_events: bool,
        log_bounces: bool,
        log_interval_us: u64,
    ) {
        eprintln!("--- intercept-bounce status ---");
        eprintln!("Debounce Threshold: {}",
            format_us(debounce_time_us));
        eprintln!("Log All Events (--log-all-events): {}", if log_all_events { "Active" } else { "Inactive" });
        eprintln!("Log Bounces (--log-bounces): {}", if log_bounces { "Active" } else { "Inactive" });
        eprintln!("Periodic Log Interval (--log-interval): {}", if log_interval_us > 0 { format!("Every {} seconds", log_interval_us / 1_000_000) } else { "Disabled".to_string() });

        eprintln!("\n--- Overall Statistics ---");
        eprintln!("Key Events Processed: {}", self.key_events_processed);
        eprintln!("Key Events Passed:    {}", self.key_events_passed);
        eprintln!("Key Events Dropped:   {}", self.key_events_dropped);
        let percentage = if self.key_events_processed > 0 {
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
        } else {
            0.0
        };
        eprintln!("Percentage Dropped:   {:.2}%", percentage);

        if !self.per_key_stats.is_empty() {
            eprintln!("\n--- Dropped Event Statistics Per Key ---");
            eprintln!("Format: Key [Name] (Code):");
            eprintln!("  State (Value): Drop Count (Bounce Time: Min / Avg / Max)");

            let mut sorted_keys: Vec<_> = self.per_key_stats.keys().collect();
            sorted_keys.sort();

            for key_code in sorted_keys {
                if let Some(stats) = self.per_key_stats.get(key_code) {
                    let key_name = get_key_name(*key_code);
                    let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

                    if total_drops_for_key > 0 {
                        eprintln!("\nKey [{}] ({}):", key_name, key_code);

                        let print_value_stats = |value_name: &str, value_code: i32, value_stats: &KeyValueStats| {
                            if value_stats.count > 0 {
                                eprint!("  {:<7} ({}): {}", value_name, value_code, value_stats.count);
                                if !value_stats.timings_us.is_empty() {
                                    let timings = &value_stats.timings_us;
                                    let min = timings.iter().min().unwrap_or(&0);
                                    let max = timings.iter().max().unwrap_or(&0);
                                    let sum: u64 = timings.iter().sum();
                                    let avg = sum as f64 / timings.len() as f64;
                                    eprintln!(" (Bounce Time: {} / {} / {})",
                                        format_us(*min),
                                        format_us(avg as u64),
                                        format_us(*max)
                                    );
                                } else {
                                    eprintln!(" (No timing data collected)");
                                }
                            }
                        };

                        print_value_stats("Press", 1, &stats.press);
                        print_value_stats("Release", 0, &stats.release);
                        print_value_stats("Repeat", 2, &stats.repeat);
                    }
                }
            }
        } else {
            eprintln!("\n--- No key events dropped ---");
        }

        if !self.per_key_passed_near_miss_timing.is_empty() {
            eprintln!("\n--- Passed Event Near-Miss Statistics (Passed within 100ms) ---");
            eprintln!("Format: Key [Name] (Code, Value): Count (Timings: Min / Avg / Max)");

            let mut sorted_near_misses: Vec<_> = self.per_key_passed_near_miss_timing.iter().collect();
            sorted_near_misses.sort_by_key(|(k, _)| *k);

            for ((code, value), timings) in sorted_near_misses {
                if !timings.is_empty() {
                    let key_name = get_key_name(*code);
                    let min = timings.iter().min().unwrap_or(&0);
                    let max = timings.iter().max().unwrap_or(&0);
                    let sum: u64 = timings.iter().sum();
                    let avg = sum as f64 / timings.len() as f64;
                    eprintln!("  Key [{}] ({}, {}): {} (Timings: {} / {} / {})",
                        key_name, code, value, timings.len(),
                        format_us(*min),
                        format_us(avg as u64),
                        format_us(*max)
                    );
                }
            }
        } else {
            eprintln!("\n--- No near-miss events recorded (< 100ms) ---");
        }

        eprintln!("----------------------------------------------------------");
    }
}

fn format_us(us: u64) -> String {
    if us >= 1000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}
