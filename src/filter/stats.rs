use std::collections::HashMap;
use crate::filter::keynames::get_key_name;
use serde::Serialize;
use colored::*;

/// Statistics for a specific key value (press/release/repeat).
#[derive(Debug, Serialize)]
pub struct KeyValueStats {
    pub count: u64,
    pub timings_us: Vec<u64>,
}

impl Default for KeyValueStats {
    fn default() -> Self {
        // Generous: pre-allocate timings vector
        KeyValueStats {
            count: 0,
            timings_us: Vec::with_capacity(1024),
        }
    }
}

/// Aggregated statistics for a specific key code.
#[derive(Debug, Serialize, Default)]
pub struct KeyStats {
    pub press: KeyValueStats,
    pub release: KeyValueStats,
    pub repeat: KeyValueStats,
}

/// Top-level statistics collector for all events.
#[derive(Debug, Serialize)]
pub struct StatsCollector {
    pub key_events_processed: u64,
    pub key_events_passed: u64,
    pub key_events_dropped: u64,
    pub per_key_stats: HashMap<u16, KeyStats>,
    pub per_key_passed_near_miss_timing: HashMap<(u16, i32), Vec<u64>>,
    pub first_event_us: Option<u64>,
    pub last_event_us: Option<u64>,
}

impl Default for StatsCollector {
    fn default() -> Self {
        StatsCollector::with_capacity(1024)
    }
}

#[allow(dead_code)]
impl StatsCollector {
    pub fn new() -> Self {
        StatsCollector::with_capacity(1024)
    }

    pub fn with_capacity(cap: usize) -> Self {
        StatsCollector {
            key_events_processed: 0,
            key_events_passed: 0,
            key_events_dropped: 0,
            per_key_stats: HashMap::with_capacity(cap),
            per_key_passed_near_miss_timing: HashMap::with_capacity(cap),
            first_event_us: None,
            last_event_us: None,
        }
    }

    pub fn record_event(&mut self, key_code: u16, key_value: i32, is_bounce: bool, bounce_diff_us: Option<u64>, event_us: u64) {
        self.key_events_processed += 1;
        self.last_event_us = Some(event_us);
        if self.first_event_us.is_none() {
            self.first_event_us = Some(event_us);
        }
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
        self.per_key_passed_near_miss_timing.entry(key).or_insert_with(|| Vec::with_capacity(128)).push(diff);
    }

    /// Print human-readable stats to stderr.
    /// Print human-readable stats to stderr.
    pub fn print_stats_to_stderr(
        &self,
        debounce_time_us: u64,
        log_all_events: bool,
        log_bounces: bool,
        log_interval_us: u64,
    ) {
        eprintln!("{}", "--- intercept-bounce status ---".bold().blue());
        eprintln!(
            "{} {}",
            "Debounce Threshold:".bold(),
            format_us(debounce_time_us).yellow()
        );
        eprintln!(
            "{} {}",
            "Log All Events (--log-all-events):".bold(),
            if log_all_events { "Active".green() } else { "Inactive".dimmed() }
        );
        eprintln!(
            "{} {}",
            "Log Bounces (--log-bounces):".bold(),
            if log_bounces { "Active".green() } else { "Inactive".dimmed() }
        );
        eprintln!(
            "{} {}",
            "Periodic Log Interval (--log-interval):".bold(),
            if log_interval_us > 0 {
                format!("Every {} seconds", log_interval_us / 1_000_000).yellow()
            } else {
                "Disabled".dimmed()
            }
        );

        eprintln!("\n{}", "--- Overall Statistics ---".bold().blue());
        eprintln!(
            "{} {}",
            "Key Events Processed:".bold(),
            self.key_events_processed
        );
        eprintln!(
            "{} {}",
            "Key Events Passed:   ".bold(),
            self.key_events_passed
        );
        eprintln!(
            "{} {}",
            "Key Events Dropped:  ".bold(),
            self.key_events_dropped
        );
        let percentage = if self.key_events_processed > 0 {
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
        } else {
            0.0
        };
        eprintln!(
            "{} {:.2}%",
            "Percentage Dropped:  ".bold(),
            percentage
        );

        if let (Some(first), Some(last)) = (self.first_event_us, self.last_event_us) {
            let duration = last.saturating_sub(first);
            eprintln!(
                "{} {}",
                "Total runtime:".bold(),
                format_us(duration).yellow()
            );
        }

        if !self.per_key_stats.is_empty() {
            eprintln!("\n{}", "--- Dropped Event Statistics Per Key ---".bold().blue());
            eprintln!("{}", "Format: Key [Name] (Code):".dimmed());
            eprintln!("{}", "  State (Value): Drop Count (Bounce Time: Min / Avg / Max)".dimmed());

            let mut sorted_keys: Vec<_> = self.per_key_stats.keys().collect();
            sorted_keys.sort();

            for key_code in sorted_keys {
                if let Some(stats) = self.per_key_stats.get(key_code) {
                    let key_name = get_key_name(*key_code);
                    let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

                    if total_drops_for_key > 0 {
                        eprintln!(
                            "\n{}",
                            format!("Key [{}] ({}):", key_name, key_code).bold().cyan()
                        );

                        let print_value_stats = |value_name: &str, value_code: i32, value_stats: &KeyValueStats| {
                            if value_stats.count > 0 {
                                eprint!(
                                    "  {:<7} ({}): {}",
                                    value_name,
                                    value_code,
                                    value_stats.count.to_string().red().bold()
                                );
                                if !value_stats.timings_us.is_empty() {
                                    let timings = &value_stats.timings_us;
                                    let min = timings.iter().min().unwrap_or(&0);
                                    let max = timings.iter().max().unwrap_or(&0);
                                    let sum: u64 = timings.iter().sum();
                                    let avg = sum as f64 / timings.len() as f64;
                                    eprintln!(
                                        " (Bounce Time: {} / {} / {})",
                                        format_us(*min).yellow(),
                                        format_us(avg as u64).yellow(),
                                        format_us(*max).yellow()
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
            eprintln!(
                "\n{}",
                "--- No key events dropped ---".green().bold()
            );
        }

        if !self.per_key_passed_near_miss_timing.is_empty() {
            eprintln!(
                "\n{}",
                "--- Passed Event Near-Miss Statistics (Passed within 100ms) ---"
                    .bold()
                    .blue()
            );
            eprintln!("{}", "Format: Key [Name] (Code, Value): Count (Timings: Min / Avg / Max)".dimmed());

            let mut sorted_near_misses: Vec<_> = self.per_key_passed_near_miss_timing.iter().collect();
            sorted_near_misses.sort_by_key(|(k, _)| *k);

            for ((code, value), timings) in sorted_near_misses {
                if !timings.is_empty() {
                    let key_name = get_key_name(*code);
                    let min = timings.iter().min().unwrap_or(&0);
                    let max = timings.iter().max().unwrap_or(&0);
                    let sum: u64 = timings.iter().sum();
                    let avg = sum as f64 / timings.len() as f64;
                    eprintln!(
                        "  Key [{}] ({}, {}): {} (Timings: {} / {} / {})",
                        key_name,
                        code,
                        value,
                        timings.len().to_string().yellow(),
                        format_us(*min).yellow(),
                        format_us(avg as u64).yellow(),
                        format_us(*max).yellow()
                    );
                }
            }
        } else {
            eprintln!(
                "\n{}",
                "--- No near-miss events recorded (< 100ms) ---"
                    .green()
                    .bold()
            );
        }

        eprintln!("{}", "----------------------------------------------------------".blue().bold());
    }

    /// Print JSON stats to the given writer (e.g. stderr).
    pub fn print_stats_json(&self, debounce_time_us: u64, log_all_events: bool, log_bounces: bool, log_interval_us: u64, mut writer: impl std::io::Write) {
        #[derive(Serialize)]
        struct Meta {
            debounce_time_us: u64,
            log_all_events: bool,
            log_bounces: bool,
            log_interval_us: u64,
        }
        #[derive(Serialize)]
        struct Output<'a> {
            meta: Meta,
            stats: &'a StatsCollector,
        }
        let meta = Meta {
            debounce_time_us,
            log_all_events,
            log_bounces,
            log_interval_us,
        };
        let output = Output { meta, stats: self };
        let _ = serde_json::to_writer_pretty(&mut writer, &output);
        let _ = writeln!(writer);
    }
}

pub fn format_us(us: u64) -> String {
    if us >= 1000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}
