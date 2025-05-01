use std::collections::HashMap;
use crate::filter::keynames::get_key_name;
use serde::Serialize;
use colored::*;
// Removed duplicate imports below

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

impl KeyValueStats {
    #[inline] // Add inline hint
    pub fn push_timing(&mut self, value: u64) {
        // Use reserve(1) to let the allocator handle growth efficiently
        // when the vector is full. This avoids potentially overallocating
        // if the capacity is often exactly met.
        if self.timings_us.len() == self.timings_us.capacity() {
             self.timings_us.reserve(1);
        }
        // Original doubling logic - kept for reference, reserve(1) is likely better
        // if self.timings_us.len() == self.timings_us.capacity() {
            // Double the capacity if full
            // let new_cap = self.timings_us.capacity().max(1024) * 2;
            // self.timings_us.reserve(new_cap - self.timings_us.capacity());
        // }
        self.timings_us.push(value);
    }
}

impl Default for KeyValueStats {
    fn default() -> Self {
        // Pre-allocate timings vector to 1024
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
// Removed Serialize derive - we will implement custom serialization logic
// in print_stats_json to handle large arrays efficiently.
#[derive(Debug)]
pub struct StatsCollector {
    pub key_events_processed: u64,
    pub key_events_passed: u64,
    pub key_events_dropped: u64,
    pub per_key_stats: Box<[KeyStats; 1024]>,
    pub per_key_passed_near_miss_timing: Box<[Vec<u64>; 3072]>,
    // Removed: These are now tracked in BounceFilter for overall duration
    // pub first_event_us: Option<u64>,
    // pub last_event_us: Option<u64>,
}

impl Default for StatsCollector {
    fn default() -> Self {
        StatsCollector::with_capacity()
    }
}

#[allow(dead_code)]
impl StatsCollector {
    #[must_use] // Add must_use hint
    pub fn new() -> Self {
        StatsCollector::with_capacity()
    }

    #[must_use] // Add must_use hint
    pub fn with_capacity() -> Self {
        // Pre-allocate all arrays and vectors
        let per_key_stats = Box::new([(); 1024].map(|_| KeyStats::default()));
        let per_key_passed_near_miss_timing = Box::new([(); 3072].map(|_| Vec::with_capacity(1024)));
        StatsCollector {
            key_events_processed: 0,
            key_events_passed: 0,
            key_events_dropped: 0,
            per_key_stats,
            per_key_passed_near_miss_timing,
            // first_event_us: None, // Removed
            // last_event_us: None, // Removed
        }
    }

    pub fn record_event(&mut self, key_code: u16, key_value: i32, is_bounce: bool, bounce_diff_us: Option<u64>, _event_us: u64) {
        self.key_events_processed += 1;
        if (key_code as usize) < 1024 {
            let key_stats = &mut self.per_key_stats[key_code as usize];
            let value_stats = match key_value {
                1 => &mut key_stats.press,
                0 => &mut key_stats.release,
                _ => &mut key_stats.repeat,
            };
            if is_bounce {
                self.key_events_dropped += 1;
                value_stats.count += 1;
                if let Some(diff) = bounce_diff_us {
                    value_stats.push_timing(diff);
                }
            } else {
                self.key_events_passed += 1;
            }
        }
    }

    pub fn record_near_miss(&mut self, key: (u16, i32), diff: u64) {
        let (key_code, key_value) = key;
        if (key_code as usize) < 1024 && (key_value as usize) < 3 {
            let idx = key_code as usize * 3 + key_value as usize;
            let vec = &mut self.per_key_passed_near_miss_timing[idx];
            if vec.len() == vec.capacity() {
                let new_cap = vec.capacity().max(1024) * 2;
                vec.reserve(new_cap - vec.capacity());
            }
            vec.push(diff);
        }
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

        let mut any_drops = false;
        for key_code in 0..1024 {
            let stats = &self.per_key_stats[key_code];
            let key_name = get_key_name(key_code as u16).on_bright_black().bright_magenta().bold();
            let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

            if total_drops_for_key > 0 {
                if !any_drops {
                    eprintln!("\n{}", "--- Dropped Event Statistics Per Key ---".on_bright_black().bold().blue().underline());
                    eprintln!("{}", "Format: Key [Name] (Code):".on_bright_black().dimmed());
                    eprintln!("{}", "  State (Value): Drop Count (Bounce Time: Min / Avg / Max)".on_bright_black().dimmed());
                    any_drops = true;
                }
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
        if !any_drops {
            eprintln!(
                "\n{}",
                "--- No key events dropped ---".on_bright_black().green().bold()
            );
        }

        let mut any_near_miss = false;
        for key_code in 0..1024 {
            for key_value in 0..3 {
                let idx = key_code * 3 + key_value;
                let timings = &self.per_key_passed_near_miss_timing[idx];
                if !timings.is_empty() {
                    if !any_near_miss {
                        eprintln!(
                            "\n{}",
                            "--- Passed Event Near-Miss Statistics (Passed within 100ms) ---"
                                .on_bright_black()
                                .bold()
                                .blue()
                                .underline()
                        );
                        eprintln!("{}", "Format: Key [Name] (Code, Value): Count (Timings: Min / Avg / Max)".on_bright_black().dimmed());
                        any_near_miss = true;
                    }
                    let key_name = get_key_name(key_code as u16).on_bright_black().bright_magenta().bold();
                    let min = timings.iter().min().unwrap_or(&0);
                    let max = timings.iter().max().unwrap_or(&0);
                    let sum: u64 = timings.iter().sum();
                    let avg = sum as f64 / timings.len() as f64;
                    eprintln!(
                        "  Key [{}] ({}, {}): {} ({}: {} / {} / {})",
                        key_name,
                        key_code.to_string().on_bright_black().bright_blue().bold(),
                        key_value.to_string().on_bright_black().bright_yellow().bold(),
                        timings.len().to_string().on_bright_black().bright_yellow().bold(),
                        "Timings".on_bright_black().bright_green().bold(),
                        format_us(*min).on_bright_black().bright_green().bold(),
                        format_us(avg as u64).on_bright_black().bright_yellow().bold(),
                        format_us(*max).on_bright_black().bright_green().bold()
                    );
                }
            }
        }
        if !any_near_miss {
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
    /// Includes runtime calculation passed from BounceFilter.
    pub fn print_stats_json(
        &self,
        debounce_time_us: u64,
        log_all_events: bool,
        log_bounces: bool,
        log_interval_us: u64,
        runtime_us: Option<u64>, // Added runtime parameter
        mut writer: impl std::io::Write,
    ) {
        // Collect only non-empty stats into HashMaps for serialization
        let mut per_key_stats_map = HashMap::new();
        for (key_code, stats) in self.per_key_stats.iter().enumerate() {
            // Only include keys that had any drops
            if stats.press.count > 0 || stats.release.count > 0 || stats.repeat.count > 0 {
                per_key_stats_map.insert(key_code as u16, stats);
            }
        }

        let mut near_miss_map = HashMap::new();
        for (idx, timings) in self.per_key_passed_near_miss_timing.iter().enumerate() {
            if !timings.is_empty() {
                let key_code = (idx / 3) as u16;
                let key_value = (idx % 3) as i32;
                near_miss_map.insert((key_code, key_value), timings);
            }
        }

        // Define structs for serialization using the collected HashMaps
        #[derive(Serialize)]
        struct FilteredStats<'a> {
            key_events_processed: u64,
            key_events_passed: u64,
            key_events_dropped: u64,
            // Use the collected HashMaps here
            per_key_stats: &'a HashMap<u16, &'a KeyStats>,
            per_key_passed_near_miss_timing: &'a HashMap<(u16, i32), &'a Vec<u64>>,
        }

        #[derive(Serialize)]
        struct Output<'a> {
            meta: Meta,
            runtime_us: Option<u64>, // Include runtime directly
            stats: FilteredStats<'a>,
        }

        let filtered_stats_data = FilteredStats {
            key_events_processed: self.key_events_processed,
            key_events_passed: self.key_events_passed,
            key_events_dropped: self.key_events_dropped,
            per_key_stats: &per_key_stats_map,
            per_key_passed_near_miss_timing: &near_miss_map,
        };

        let meta = Meta {
            debounce_time_us,
            log_all_events,
            log_bounces,
            log_interval_us,
        };

        let output = Output {
            meta,
            runtime_us, // Use the passed runtime
            stats: filtered_stats_data,
        };

        // Write the JSON output
        let _ = serde_json::to_writer_pretty(&mut writer, &output);
        // Add a newline for better formatting in the terminal
        let _ = writeln!(writer);
    }
}

#[inline] // Add inline hint
pub fn format_us(us: u64) -> String {
    if us >= 1000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}
