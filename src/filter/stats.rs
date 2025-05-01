use std::collections::HashMap;
use crate::filter::keynames::get_key_name;
use serde::Serialize;
use colored::*;
// Removed duplicate imports below
// use std::collections::HashMap;
// use crate::filter::keynames::get_key_name;
// use serde::Serialize;

/// Metadata included in JSON statistics output.
#[derive(Serialize)]
pub struct Meta {
    pub debounce_time_us: u64,
    pub log_all_events: bool,
    pub log_bounces: bool,
    pub log_interval_us: u64,
}


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
    // Removed: These are now tracked in BounceFilter for overall duration
    // pub first_event_us: Option<u64>,
    // pub last_event_us: Option<u64>,
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
            // first_event_us: None, // Removed
            // last_event_us: None, // Removed
        }
    }

    pub fn record_event(&mut self, key_code: u16, key_value: i32, is_bounce: bool, bounce_diff_us: Option<u64>, _event_us: u64) {
        self.key_events_processed += 1;
        // Removed timestamp tracking from StatsCollector
        // self.last_event_us = Some(event_us);
        // if self.first_event_us.is_none() {
        //     self.first_event_us = Some(event_us);
        // }
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
        eprintln!("{}", "--- intercept-bounce status ---".on_bright_black().bold().blue().underline());
        eprintln!(
            "{} {}",
            "Debounce Threshold:".on_bright_black().bold().bright_yellow(),
            format_us(debounce_time_us).on_bright_black().bright_yellow().bold()
        );
        eprintln!(
            "{} {}",
            "Log All Events (--log-all-events):".on_bright_black().bold().bright_cyan(),
            if log_all_events { "Active".on_green().black().bold() } else { "Inactive".on_bright_black().dimmed() }
        );
        eprintln!(
            "{} {}",
            "Log Bounces (--log-bounces):".on_bright_black().bold().bright_red(),
            if log_all_events {
                "Overridden by --log-all-events".on_bright_black().bright_yellow().bold()
            } else if log_bounces {
                "Active".on_red().white().bold()
            } else {
                "Inactive".on_bright_black().dimmed()
            }
        );
        eprintln!(
            "{} {}",
            "Periodic Log Interval (--log-interval):".on_bright_black().bold().bright_magenta(),
            if log_interval_us > 0 {
                format!("Every {} seconds", log_interval_us / 1_000_000).on_bright_black().bright_magenta().bold()
            } else {
                "Disabled".on_bright_black().dimmed()
            }
        );

        eprintln!("\n{}", "--- Overall Statistics ---".on_bright_black().bold().blue().underline());
        eprintln!(
            "{} {}",
            "Key Events Processed:".on_bright_black().bold().bright_white(),
            self.key_events_processed.to_string().on_bright_black().bright_white().bold()
        );
        eprintln!(
            "{} {}",
            "Key Events Passed:   ".on_bright_black().bold().bright_green(),
            self.key_events_passed.to_string().on_bright_black().bright_green().bold()
        );
        eprintln!(
            "{} {}",
            "Key Events Dropped:  ".on_bright_black().bold().bright_red(),
            self.key_events_dropped.to_string().on_bright_black().bright_red().bold()
        );
        let percentage = if self.key_events_processed > 0 {
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
        } else {
            0.0
        };
        eprintln!(
            "{} {:.2}%",
            "Percentage Dropped:  ".on_bright_black().bold().bright_red(),
            percentage
        );

        // Removed runtime calculation from here. It's now calculated and printed
        // in BounceFilter::print_stats using overall timestamps.
        // if let (Some(first), Some(last)) = (self.first_event_us, self.last_event_us) {
        //     let duration = last.saturating_sub(first);
        //     eprintln!(
        //         "{} {}",
        //         "Total runtime:".on_bright_black().bold().bright_yellow(),
        //         format_us(duration).on_bright_black().bright_yellow().bold()
        //     );
        // }

        if !self.per_key_stats.is_empty() {
            eprintln!("\n{}", "--- Dropped Event Statistics Per Key ---".on_bright_black().bold().blue().underline());
            eprintln!("{}", "Format: Key [Name] (Code):".on_bright_black().dimmed());
            eprintln!("{}", "  State (Value): Drop Count (Bounce Time: Min / Avg / Max)".on_bright_black().dimmed());

            let mut sorted_keys: Vec<_> = self.per_key_stats.keys().collect();
            sorted_keys.sort();

            for key_code in sorted_keys {
                if let Some(stats) = self.per_key_stats.get(key_code) {
                    let key_name = get_key_name(*key_code).on_bright_black().bright_magenta().bold();
                    let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

                    if total_drops_for_key > 0 {
                        eprintln!(
                            "\n{}",
                            format!("Key [{}] ({}):", key_name, key_code).on_bright_black().bold().cyan()
                        );

                        let print_value_stats = |value_name: &str, value_code: i32, value_stats: &KeyValueStats| {
                            if value_stats.count > 0 {
                                eprint!(
                                    "  {:<7} ({}): {}",
                                    value_name.on_bright_black().bold().bright_yellow(),
                                    value_code.to_string().on_bright_black().bright_blue().bold(),
                                    value_stats.count.to_string().on_red().white().bold()
                                );
                                if !value_stats.timings_us.is_empty() {
                                    let timings = &value_stats.timings_us;
                                    let min = timings.iter().min().unwrap_or(&0);
                                    let max = timings.iter().max().unwrap_or(&0);
                                    let sum: u64 = timings.iter().sum();
                                    let avg = sum as f64 / timings.len() as f64;
                                    eprintln!(
                                        " ({}: {} / {} / {})",
                                        "Bounce Time".on_bright_black().bright_red().bold(),
                                        format_us(*min).on_bright_black().bright_red().bold(),
                                        format_us(avg as u64).on_bright_black().bright_yellow().bold(),
                                        format_us(*max).on_bright_black().bright_red().bold()
                                    );
                                } else {
                                    eprintln!("{}", " (No timing data collected)".on_bright_black().dimmed());
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
                "--- No key events dropped ---".on_bright_black().green().bold()
            );
        }

        if !self.per_key_passed_near_miss_timing.is_empty() {
            eprintln!(
                "\n{}",
                "--- Passed Event Near-Miss Statistics (Passed within 100ms) ---"
                    .on_bright_black()
                    .bold()
                    .blue()
                    .underline()
            );
            eprintln!("{}", "Format: Key [Name] (Code, Value): Count (Timings: Min / Avg / Max)".on_bright_black().dimmed());

            let mut sorted_near_misses: Vec<_> = self.per_key_passed_near_miss_timing.iter().collect();
            sorted_near_misses.sort_by_key(|(k, _)| *k);

            for ((code, value), timings) in sorted_near_misses {
                if !timings.is_empty() {
                    let key_name = get_key_name(*code).on_bright_black().bright_magenta().bold();
                    let min = timings.iter().min().unwrap_or(&0);
                    let max = timings.iter().max().unwrap_or(&0);
                    let sum: u64 = timings.iter().sum();
                    let avg = sum as f64 / timings.len() as f64;
                    eprintln!(
                        "  Key [{}] ({}, {}): {} ({}: {} / {} / {})",
                        key_name,
                        code.to_string().on_bright_black().bright_blue().bold(),
                        value.to_string().on_bright_black().bright_yellow().bold(),
                        timings.len().to_string().on_bright_black().bright_yellow().bold(),
                        "Timings".on_bright_black().bright_green().bold(),
                        format_us(*min).on_bright_black().bright_green().bold(),
                        format_us(avg as u64).on_bright_black().bright_yellow().bold(),
                        format_us(*max).on_bright_black().bright_green().bold()
                    );
                }
            }
        } else {
            eprintln!(
                "\n{}",
                "--- No near-miss events recorded (< 100ms) ---"
                    .on_bright_black()
                    .green()
                    .bold()
            );
        }

        eprintln!("{}", "----------------------------------------------------------".on_bright_black().blue().bold());
    }

    /// Print JSON stats to the given writer (e.g. stderr).
    pub fn print_stats_json(&self, debounce_time_us: u64, log_all_events: bool, log_bounces: bool, log_interval_us: u64, mut writer: impl std::io::Write) {
        // Meta struct moved outside this function

        // Removed first_event_us and last_event_us from JSON output as well
        #[derive(Serialize)]
        struct FilteredStats<'a> {
            key_events_processed: u64,
            key_events_passed: u64,
            key_events_dropped: u64,
            per_key_stats: &'a HashMap<u16, KeyStats>,
            per_key_passed_near_miss_timing: &'a HashMap<(u16, i32), Vec<u64>>,
        }

        #[derive(Serialize)]
        struct Output<'a> {
            meta: Meta,
            stats: FilteredStats<'a>,
            // Add runtime here if needed, passed from BounceFilter
            // runtime_us: Option<u64>,
        }

        let filtered_stats = FilteredStats {
            key_events_processed: self.key_events_processed,
            key_events_passed: self.key_events_passed,
            key_events_dropped: self.key_events_dropped,
            per_key_stats: &self.per_key_stats,
            per_key_passed_near_miss_timing: &self.per_key_passed_near_miss_timing,
        };

        let meta = Meta {
            debounce_time_us,
            log_all_events,
            log_bounces,
            log_interval_us,
        };
        // The caller (main.rs) now constructs the final JSON output including runtime
        let output = Output { meta, stats: filtered_stats };
        let _ = serde_json::to_writer_pretty(&mut writer, &output);
        // Remove the writeln! here, let the caller handle final newline if needed
        // let _ = writeln!(writer);
    }
}

pub fn format_us(us: u64) -> String {
    if us >= 1000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}
