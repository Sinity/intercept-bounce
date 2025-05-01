// This module defines the StatsCollector struct and related types
// used by the logger thread to accumulate and report statistics.

use crate::filter::keynames::get_key_name;
use crate::logger::EventInfo; // Use EventInfo from logger module
use colored::*;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write; // Need Write for print_stats_json

/// Metadata included in JSON statistics output, providing context.
#[derive(Serialize, Clone, Debug)]
pub struct Meta {
    pub debounce_time_us: u64,
    pub near_miss_threshold_us: u64,
    pub log_all_events: bool,
    pub log_bounces: bool,
    pub log_interval_us: u64,
}

/// Statistics for a specific key value state (press/release/repeat).
/// Holds the count of dropped events and the timing differences for those drops.
#[derive(Debug, Serialize, Clone, Default)]
pub struct KeyValueStats {
    pub count: u64,
    // Stores the microsecond difference between a dropped event and the previous passed event.
    pub timings_us: Vec<u64>,
}

impl KeyValueStats {
    /// Adds a timing value to the vector, resizing if necessary.
    #[inline]
    pub fn push_timing(&mut self, value: u64) {
        if self.timings_us.len() == self.timings_us.capacity() {
             self.timings_us.reserve(1);
        }
        self.timings_us.push(value);
    }
}

/// Aggregated statistics for a specific key code, containing stats for each value state.
#[derive(Debug, Serialize, Clone, Default)]
pub struct KeyStats {
    pub press: KeyValueStats,
    pub release: KeyValueStats,
    pub repeat: KeyValueStats,
}

/// Top-level statistics collector. Owned and managed by the logger thread.
/// Accumulates counts, drop timings, and near-miss timings for all processed events.
#[derive(Debug, Clone)]
pub struct StatsCollector {
    /// Total count of key events processed (passed or dropped).
    pub key_events_processed: u64,
    /// Total count of key events that passed the filter.
    pub key_events_passed: u664,
    /// Total count of key events dropped by the filter.
    pub key_events_dropped: u64,
    /// Holds aggregated drop stats per key code. Uses a fixed-size array for O(1) lookup.
    pub per_key_stats: Box<[KeyStats; 1024]>,
    /// Holds near-miss timings for passed events. Indexed by `keycode * 3 + value`.
    pub per_key_passed_near_miss_timing: Box<[Vec<u64>; 3072]>,
}

// Implement Default to allow std::mem::take in logger.
impl Default for StatsCollector {
    fn default() -> Self {
        StatsCollector::with_capacity()
    }
}

impl StatsCollector {
    /// Creates a new StatsCollector with pre-allocated storage.
    #[must_use]
    pub fn with_capacity() -> Self {
        let per_key_stats = Box::new([(); 1024].map(|_| KeyStats::default()));
        let per_key_passed_near_miss_timing =
            Box::new([(); 3072].map(|_| Vec::with_capacity(1024)));
        StatsCollector {
            key_events_processed: 0,
            key_events_passed: 0,
            key_events_dropped: 0,
            per_key_stats,
            per_key_passed_near_miss_timing,
        }
    }

    /// Updates statistics based on information about a processed event,
    /// using the provided configuration.
    /// This is the central method for stats accumulation, called by the logger thread.
    pub fn record_event_info_with_config(&mut self, info: &EventInfo, config: &crate::config::Config) {
         // Use the is_key_event helper from the event module
        use crate::event::is_key_event;

        // Only process EV_KEY events for these statistics.
        if !is_key_event(&info.event) {
            return;
        }

        self.key_events_processed += 1;

        let key_code_idx = info.event.code as usize;
        let key_value_idx = info.event.value as usize;

        if info.is_bounce {
            self.key_events_dropped += 1;
            if key_code_idx < 1024 && key_value_idx < 3 {
                 let value_stats = match info.event.value { 1 => &mut self.per_key_stats[key_code_idx].press, 0 => &mut self.per_key_stats[key_code_idx].release, _ => &mut self.per_key_stats[key_code_idx].repeat, };
                 if let Some(diff) = info.diff_us { value_stats.count += 1; value_stats.push_timing(diff); }
            }
        } else { // Event passed the filter.
            self.key_events_passed += 1;
            if let Some(last_us) = info.last_passed_us {
                if let Some(diff) = info.event_us.checked_sub(last_us) {
                    // Check if the difference is within the near-miss window (debounce_time <= diff <= threshold)
                    // The filter ensures diff >= debounce_time for passed events.
                    // Here, we check against the near-miss threshold.
                    if diff <= config.near_miss_threshold_us {
                        self.record_near_miss((info.event.code, info.event.value), diff);
                    }
                }
            }
        }
    }

    /// Records the timing difference for a passed event that was a near miss.
    /// Called internally by `record_event_info`.
    fn record_near_miss(&mut self, key: (u16, i32), diff: u64) {
        let (key_code, key_value) = key;
        // Check bounds before calculating index.
        if (key_code as usize) < 1024 && (key_value as usize) < 3 {
            // Calculate the flat index for the per_key_passed_near_miss_timing array.
            let idx = key_code as usize * 3 + key_value as usize;
            let vec = &mut self.per_key_passed_near_miss_timing[idx];
            // Use reserve(1) for potentially better allocation strategy than doubling.
            if vec.len() == vec.capacity() {
                vec.reserve(1);
            }
            vec.push(diff);
        }
    }

    /// Prints human-readable statistics summary to stderr.
    pub fn print_stats_to_stderr(
        &self,
        config: &crate::config::Config,
    ) {
        let log_all_events = config.log_all_events;
        let log_bounces = config.log_bounces;
        let log_interval_us = config.log_interval_us;

        eprintln!("--- intercept-bounce status ---");
        eprintln!("Debounce Threshold: {}", format_us(config.debounce_us));
        eprintln!("Near-Miss Threshold: {}", format_us(config.near_miss_threshold_us));
        eprintln!("Log All Events (--log-all-events): {}", if log_all_events { "Active" } else { "Inactive" });
        eprintln!("Log Bounces (--log-bounces): {}",
            if log_all_events { "Overridden by --log-all-events" }
            else if log_bounces { "Active" }
            else { "Inactive" }
        );
        eprintln!("Periodic Log Interval (--log-interval): {}",
            if log_interval_us > 0 { format!("Every {} seconds", log_interval_us / 1_000_000) }
            else { "Disabled" }
        );

        eprintln!("\n--- Overall Statistics ---");
        eprintln!("Key Events Processed: {}", self.key_events_processed);
        eprintln!("Key Events Passed:   {}", self.key_events_passed);
        eprintln!("Key Events Dropped:  {}", self.key_events_dropped);
        let percentage = if self.key_events_processed > 0 {
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
        } else {
            0.0
        };
        eprintln!("Percentage Dropped:  {:.2}%", percentage);

        let mut any_drops = false;
        for key_code in 0..self.per_key_stats.len() {
            let stats = &self.per_key_stats[key_code];
            let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

            if total_drops_for_key > 0 {
                if !any_drops {
                    eprintln!("\n--- Dropped Event Statistics Per Key ---");
                    eprintln!("Format: Key [Name] (Code):");
                    eprintln!("  State (Value): Drop Count (Bounce Time: Min / Avg / Max)");
                    any_drops = true;
                }

                let key_name = get_key_name(key_code as u16);
                 eprintln!("\nKey [{}] ({}):", key_name, key_code);

                let print_value_stats = |value_name: &str, value_code: i32, value_stats: &KeyValueStats| {
                    if value_stats.count > 0 {
                        eprint!("  {:<7} ({}): {}", value_name, value_code, value_stats.count);
                        if !value_stats.timings_us.is_empty() {
                            let timings = &value_stats.timings_us;
                            let min = timings.iter().min().copied().unwrap_or(0);
                            let max = timings.iter().max().copied().unwrap_or(0);
                            let sum: u64 = timings.iter().sum();
                            let avg = if !timings.is_empty() { sum as f64 / timings.len() as f64 } else { 0.0 };
                            eprintln!(" (Bounce Time: {} / {} / {})", format_us(min), format_us(avg as u64), format_us(max));
                        } else {
                            eprintln!(" (No timing data)");
                        }
                    }
                };

                print_value_stats("Press", 1, &stats.press);
                print_value_stats("Release", 0, &stats.release);
                print_value_stats("Repeat", 2, &stats.repeat);
            }
        }
        if !any_drops {
            eprintln!("\n--- No key events dropped ---");
        }

        let mut any_near_miss = false;
        for idx in 0..self.per_key_passed_near_miss_timing.len() {
            let timings = &self.per_key_passed_near_miss_timing[idx];
            if !timings.is_empty() {
                 if !any_near_miss {
                    eprintln!("\n--- Passed Event Near-Miss Statistics (Passed within {}) ---", format_us(config.near_miss_threshold_us));
                    eprintln!("Format: Key [Name] (Code, Value): Count (Near-Miss Time: Min / Avg / Max)");
                    any_near_miss = true;
                }

                let key_code = (idx / 3) as u16;
                let key_value = (idx % 3) as i32;
                let key_name = get_key_name(key_code);

                let min = timings.iter().min().copied().unwrap_or(0);
                let max = timings.iter().max().copied().unwrap_or(0);
                let sum: u64 = timings.iter().sum();
                let avg = if !timings.is_empty() { sum as f64 / timings.len() as f64 } else { 0.0 };

                eprintln!(
                    "  Key [{}] ({}, {}): {} (Near-Miss Time: {} / {} / {})",
                    key_name,
                    key_code,
                    key_value,
                    timings.len(),
                    format_us(min),
                    format_us(avg as u64),
                    format_us(max)
                );
            }
        }
         if !any_near_miss {
            eprintln!("\n--- No near-miss events recorded (< {}) ---", format_us(config.near_miss_threshold_us));
        }

        eprintln!("----------------------------------------------------------");
    }

    /// Prints statistics in JSON format to the given writer.
    /// Includes runtime provided externally (calculated in main thread).
    pub fn print_stats_json(
        &self,
        config: &crate::config::Config,
        runtime_us: Option<u64>,
        mut writer: impl Write,
    ) {
        let debounce_time_us = config.debounce_us;
        let near_miss_threshold_us = config.near_miss_threshold_us;
        let log_all_events = config.log_all_events;
        let log_bounces = config.log_bounces;
        let log_interval_us = config.log_interval_us;

        let mut per_key_stats_map = HashMap::new();
        for (key_code, stats) in self.per_key_stats.iter().enumerate() {
            if stats.press.count > 0 || stats.release.count > 0 || stats.repeat.count > 0 {
                per_key_stats_map.insert(key_code as u16, stats);
            }
        }

        let mut near_miss_map = HashMap::new();
        for (idx, timings) in self.per_key_passed_near_miss_timing.iter().enumerate() {
            if !timings.is_empty() {
                let key_code = (idx / 3) as u16;
                let key_value = (idx % 3) as i32;
                let key_str = format!("[{},{}]", key_code, key_value);
                near_miss_map.insert(key_str, timings);
            }
        }

        #[derive(Serialize)]
        struct FilteredStatsData<'a> {
            key_events_processed: u64,
            key_events_passed: u64,
            key_events_dropped: u64,
            per_key_stats: &'a HashMap<u16, &'a KeyStats>,
            per_key_passed_near_miss_timing: &'a HashMap<String, &'a Vec<u64>>,
        }

        #[derive(Serialize)]
        struct JsonOutput<'a> {
            meta: Meta,
            runtime_us: Option<u64>,
            stats: FilteredStatsData<'a>,
        }

        let filtered_stats_data = FilteredStatsData {
            key_events_processed: self.key_events_processed,
            key_events_passed: self.key_events_passed,
            key_events_dropped: self.key_events_dropped,
            per_key_stats: &per_key_stats_map,
            per_key_passed_near_miss_timing: &near_miss_map,
        };

        let meta = Meta {
            debounce_time_us,
            near_miss_threshold_us,
            log_all_events,
            log_bounces,
            log_interval_us,
        };

        let output = JsonOutput {
            meta,
            runtime_us,
            stats: filtered_stats_data,
        };

        let _ = serde_json::to_writer_pretty(&mut writer, &output);
        let _ = writeln!(writer);
    }
}

/// Formats a duration in microseconds into a human-readable string (µs or ms).
#[inline]
pub fn format_us(us: u64) -> String {
    if us < 1000 {
        format!("{} µs", us)
    } else if us < 1_000_000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{:.3} s", us as f64 / 1_000_000.0)
    }
}
