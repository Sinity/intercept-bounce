// This module defines the StatsCollector struct and related types
// used by the logger thread to accumulate and report statistics.
use crate::filter::{FILTER_MAP_SIZE, NUM_KEY_STATES};

use crate::filter::keynames::{get_key_name, get_value_name};
use crate::logger::EventInfo;
use crate::util;
use serde::Serialize;
use std::io::Write;
use std::time::Duration;

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
    /// Total events processed (passed + dropped) for this specific key state.
    pub total_processed: u64,
    /// Count of events that passed the filter for this specific key state.
    pub passed_count: u64,
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

/// Structure for serializing per-key drop statistics in JSON.
#[derive(Serialize, Debug)]
struct PerKeyStatsJson<'a> {
    key_code: u16,
    key_name: &'static str,
    total_processed: u64,
    total_dropped: u64,
    drop_percentage: f64,
    stats: &'a KeyStats, // Contains press/release/repeat details
}

/// Structure for serializing near-miss statistics in JSON.
#[derive(Serialize, Debug)]
struct NearMissJson<'a> {
    key_code: u16,
    key_value: i32,
    key_name: &'static str,
    value_name: &'static str,
    count: usize,
    timings_us: &'a Vec<u64>,
    // Add min/avg/max directly to JSON object
    min_us: u64,
    avg_us: u64,
    max_us: u64,
}

/// Top-level statistics collector. Owned and managed by the logger thread.
/// Accumulates counts, drop timings, and near-miss timings for all processed events.
#[derive(Debug, Clone)]
pub struct StatsCollector {
    /// Total count of key events processed (passed or dropped).
    pub key_events_processed: u64,
    /// Total count of key events that passed the filter.
    pub key_events_passed: u64,
    /// Total count of key events dropped by the filter.
    pub key_events_dropped: u64,
    /// Holds aggregated drop stats per key code. Uses a fixed-size array for O(1) lookup.
    pub per_key_stats: Box<[KeyStats; FILTER_MAP_SIZE]>,
    /// Holds near-miss timings for passed events. Indexed by `keycode * 3 + value`.
    pub per_key_passed_near_miss_timing: Box<[Vec<u64>; FILTER_MAP_SIZE * NUM_KEY_STATES]>,
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
        // Allocate the arrays on the heap using Box::new
        let per_key_stats = Box::new([(); FILTER_MAP_SIZE].map(|_| KeyStats::default()));
        let per_key_passed_near_miss_timing =
            Box::new([(); FILTER_MAP_SIZE * NUM_KEY_STATES].map(|_| Vec::with_capacity(1024))); // Assuming initial capacity is desired

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
    pub fn record_event_info_with_config(
        &mut self,
        info: &EventInfo,
        config: &crate::config::Config,
    ) {
        use crate::event::is_key_event;

        // Only process EV_KEY events for these statistics.
        if !is_key_event(&info.event) {
            return;
        }

        self.key_events_processed += 1;

        // Get mutable access to the specific KeyValueStats for this event, if valid
        let key_code_idx = info.event.code as usize;
        let key_value_idx = info.event.value as usize;
        let mut maybe_value_stats = if key_code_idx < FILTER_MAP_SIZE && key_value_idx < NUM_KEY_STATES {
            Some(match info.event.value {
                1 => &mut self.per_key_stats[key_code_idx].press,
                0 => &mut self.per_key_stats[key_code_idx].release,
                _ => &mut self.per_key_stats[key_code_idx].repeat,
            })
        } else {
            None
        };

        // Increment total processed count if we found valid stats
        if let Some(value_stats) = maybe_value_stats.as_mut() {
            value_stats.total_processed += 1;
        }

        // Handle bounce/pass logic
        if info.is_bounce {
            self.key_events_dropped += 1;
            // Increment drop count and record timing if we found valid stats
            if let Some(value_stats) = maybe_value_stats {
                if let Some(diff) = info.diff_us {
                    value_stats.count += 1; // Increment drop count for this state
                    value_stats.push_timing(diff);
                }
            }
        } else {
            // Event passed the filter.
            self.key_events_passed += 1;
            // Increment passed count if we found valid stats
            if let Some(value_stats) = maybe_value_stats.as_mut() {
                value_stats.passed_count += 1;
            }
            // Check for near-miss only on passed events if we found valid stats
            if maybe_value_stats.is_some() { // This check is redundant now, but harmless
                if let Some(last_us) = info.last_passed_us {
                    if let Some(diff) = info.event_us.checked_sub(last_us) {
                        // Check if the difference is within the near-miss window (debounce_time <= diff <= threshold)
                        // The filter ensures diff >= debounce_time for passed events.
                        // Here, we check against the near-miss threshold.
                        if diff <= config.near_miss_threshold_us() {
                            self.record_near_miss((info.event.code, info.event.value), diff);
                        }
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
        if (key_code as usize) < FILTER_MAP_SIZE && (key_value as usize) < NUM_KEY_STATES {
            // Calculate the flat index for the per_key_passed_near_miss_timing array.
            let idx = key_code as usize * NUM_KEY_STATES + key_value as usize;
            let vec = &mut self.per_key_passed_near_miss_timing[idx];
            // Use reserve(1) for potentially better allocation strategy than doubling.
            if vec.len() == vec.capacity() {
                vec.reserve(1);
            }
            vec.push(diff);
        }
    }

    /// Formats human-readable statistics summary and writes it to the provided writer.
    /// Returns an io::Result to handle potential write errors.
    pub fn format_stats_human_readable(
        &self,
        config: &crate::config::Config,
        report_type: &str,
        mut writer: impl Write, // Accept a generic writer
    ) -> std::io::Result<()> {

        writeln!(writer, "\n--- Overall Statistics ({report_type}) ---")?;
        writeln!(
            writer,
            "Key Events Processed: {}",
            self.key_events_processed
        )?;
        writeln!(writer, "Key Events Passed:   {}", self.key_events_passed)?;
        writeln!(writer, "Key Events Dropped:  {}", self.key_events_dropped)?;
        let percentage = if self.key_events_processed > 0 {
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
        } else {
            0.0
        };
        writeln!(writer, "Percentage Dropped:  {percentage:.2}%")?;

        let mut any_drops = false;
        for key_code in 0..self.per_key_stats.len() {
            let stats = &self.per_key_stats[key_code];
            let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

            if total_drops_for_key > 0 {
                if !any_drops {
                    writeln!(writer, "\n--- Dropped Event Statistics Per Key ---")?;
                    writeln!(writer, "Format: Key [Name] (Code):")?;
                    writeln!(
                        writer, // Updated format description
                        "  State (Value): Processed: <count>, Passed: <count>, Dropped: <count> (<rate>%) (Bounce Time: Min / Avg / Max)"
                    )?;
                    any_drops = true;
                }

                let key_name = get_key_name(key_code as u16);
                writeln!(writer, "\nKey [{key_name}] ({key_code}):")?;
                // Calculate total processed for this key
                let total_processed_for_key =
                    stats.press.total_processed + stats.release.total_processed + stats.repeat.total_processed;
                // Calculate total passed for this key
                let total_passed_for_key =
                    stats.press.passed_count + stats.release.passed_count + stats.repeat.passed_count;
                // Calculate overall drop percentage for this key
                let key_drop_percentage = if total_processed_for_key > 0 { // Base percentage on total processed
                    (total_drops_for_key as f64 / total_processed_for_key as f64) * 100.0
                } else {
                    0.0
                };
                writeln!(
                    writer, // Updated summary line format
                    "  Total Processed: {total_processed_for_key}, Passed: {total_passed_for_key}, Dropped: {total_drops_for_key} ({key_drop_percentage:.2}%)"
                )?;

                // Use a closure that captures writer mutably
                let mut print_value_stats = |value_name: &str,
                                             value_code: i32,
                                             value_stats: &KeyValueStats|
                 -> std::io::Result<()> {
                    if value_stats.count > 0 || value_stats.passed_count > 0 { // Print if any events (passed or dropped) for this state
                        // Calculate drop rate for this specific state
                        let drop_rate = if value_stats.total_processed > 0 {
                            (value_stats.count as f64 / value_stats.total_processed as f64) * 100.0
                        } else {
                            0.0
                        };
                        // Updated detail line format
                        write!(
                            writer, // Use write! not writeln!
                            "  {:<7} ({}): Processed: {}, Passed: {}, Dropped: {} ({:.2}%)",
                            value_name, value_code, value_stats.total_processed, value_stats.passed_count, value_stats.count, drop_rate)?;
                        if !value_stats.timings_us.is_empty() {
                            let timings = &value_stats.timings_us;
                            let min = timings.iter().min().copied().unwrap_or(0);
                            let max = timings.iter().max().copied().unwrap_or(0);
                            let sum: u64 = timings.iter().sum();
                            let avg = if !timings.is_empty() {
                                sum as f64 / timings.len() as f64
                            } else {
                                0.0
                            };
                            writeln!(
                                writer,
                                " (Bounce Time: {} / {} / {})",
                                util::format_us(min),
                                util::format_us(avg as u64),
                                util::format_us(max)
                            )?;
                        } else {
                            writeln!(writer, "")?; // Newline if no timing data
                        }
                    }
                    Ok(()) // Return Ok from the closure
                };

                print_value_stats("Press", 1, &stats.press)?;
                print_value_stats("Release", 0, &stats.release)?;
                print_value_stats("Repeat", 2, &stats.repeat)?;
            }
        }
        if !any_drops {
            writeln!(writer, "\n--- No key events dropped ---")?;
        }

        let mut any_near_miss = false;
        for idx in 0..self.per_key_passed_near_miss_timing.len() {
            let timings = &self.per_key_passed_near_miss_timing[idx];
            if !timings.is_empty() {
                if !any_near_miss {
                    writeln!(
                        writer,
                        "\n--- Passed Event Near-Miss Statistics (Passed within {}) ---",
                        util::format_duration(config.near_miss_threshold())
                    )?;
                    writeln!(
                        writer,
                        "Format: Key [Name] (Code, Value): Count (Near-Miss Time: Min / Avg / Max)"
                    )?;
                    any_near_miss = true;
                }

                let key_code = (idx / NUM_KEY_STATES) as u16;
                let key_value = (idx % NUM_KEY_STATES) as i32;
                let key_name = get_key_name(key_code);
                let value_name = get_value_name(key_value);

                let min = timings.iter().min().copied().unwrap_or(0);
                let max = timings.iter().max().copied().unwrap_or(0);
                let sum: u64 = timings.iter().sum();
                let avg = if !timings.is_empty() {
                    sum as f64 / timings.len() as f64
                } else {
                    0.0
                };

                writeln!(
                    writer,
                    "  Key [{}] ({}, {}): {} (Near-Miss Time: {} / {} / {})",
                    key_name,
                    key_code,
                    key_value,
                    timings.len(),
                    util::format_us(min),
                    util::format_us(avg as u64),
                    util::format_us(max)
                )?;
            }
        }
        if !any_near_miss {
            writeln!(
                writer,
                "\n--- No near-miss events recorded (< {}) ---",
                util::format_duration(config.near_miss_threshold())
            )?;
        }

        writeln!(
            writer,
            "----------------------------------------------------------"
        )?;
        Ok(()) // Return Ok(()) at the end of the function
    }

    /// Prints human-readable statistics summary to stderr by calling format_stats_human_readable.
    pub fn print_stats_to_stderr(&self, config: &crate::config::Config, report_type: &str) {
        // Ignore potential write errors when writing to stderr, as there's not much we can do.
        let _ =
            self.format_stats_human_readable(config, report_type, &mut std::io::stderr().lock());
    }

    /// Prints statistics in JSON format to the given writer.
    /// Includes runtime provided externally (calculated in main thread).
    pub fn print_stats_json(
        &self,
        config: &crate::config::Config,
        runtime_us: Option<u64>,
        report_type: &str,
        mut writer: impl Write,
    ) {

        // --- Intermediate Structs for JSON Serialization ---
        #[derive(Serialize)]
        struct KeyValueStatsJson<'a> {
            total_processed: u64,
            passed_count: u64,
            dropped_count: u64, // Renamed from 'count' for clarity in JSON
            drop_rate: f64,
            timings_us: &'a Vec<u64>, // Reference original timings
        }

        #[derive(Serialize)]
        struct KeyStatsJson<'a> {
            press: KeyValueStatsJson<'a>,
            release: KeyValueStatsJson<'a>,
            repeat: KeyValueStatsJson<'a>, // Keep repeat for structure consistency
        }

        // Modify PerKeyStatsJson to use KeyStatsJson and remove lifetime
        #[derive(Serialize)]
        struct PerKeyStatsJson {
            key_code: u16,
            key_name: &'static str,
            total_processed: u64,
            total_dropped: u64,
            drop_percentage: f64, // Overall drop % for the key
            stats: KeyStatsJson, // Use the new struct holding detailed stats
        }

        // --- Prepare Per-Key Drop Stats for JSON ---
        let mut per_key_stats_json_vec = Vec::new();
        for (key_code_usize, stats) in self.per_key_stats.iter().enumerate() {
            let total_processed_for_key = stats.press.total_processed
                + stats.release.total_processed
                + stats.repeat.total_processed;
            let total_dropped_for_key =
                stats.press.count + stats.release.count + stats.repeat.count; // Include repeat drops here for overall key drop count

            if total_processed_for_key > 0 { // Include keys with any activity (passed or dropped)
                let key_code = key_code_usize as u16;
                let key_name = get_key_name(key_code);
                let drop_percentage = if total_processed_for_key > 0 {
                    (total_dropped_for_key as f64 / total_processed_for_key as f64) * 100.0
                } else {
                    0.0
                };

                // Helper closure to create KeyValueStatsJson
                let create_kv_stats_json = |kv_stats: &KeyValueStats| -> KeyValueStatsJson {
                    let drop_rate = if kv_stats.total_processed > 0 {
                        (kv_stats.count as f64 / kv_stats.total_processed as f64) * 100.0
                    } else {
                        0.0
                    };
                    KeyValueStatsJson {
                        total_processed: kv_stats.total_processed,
                        passed_count: kv_stats.passed_count,
                        dropped_count: kv_stats.count, // Use original drop count field
                        drop_rate,
                        timings_us: &kv_stats.timings_us,
                    }
                };

                // Populate the detailed stats structure for JSON
                let detailed_stats_json = KeyStatsJson {
                    press: create_kv_stats_json(&stats.press),
                    release: create_kv_stats_json(&stats.release),
                    // Repeat stats are included for structure, rate will be 0.0
                    repeat: create_kv_stats_json(&stats.repeat),
                };

                per_key_stats_json_vec.push(PerKeyStatsJson {
                    key_code,
                    key_name,
                    total_processed: total_processed_for_key,
                    total_dropped: total_dropped_for_key,
                    drop_percentage,
                    stats: detailed_stats_json, // Use the new detailed struct
                });
            }
        }

        // --- Prepare Near-Miss Stats for JSON ---
        let mut near_miss_json_vec = Vec::new();
        for (idx, timings) in self.per_key_passed_near_miss_timing.iter().enumerate() {
            if !timings.is_empty() {
                let key_code = (idx / NUM_KEY_STATES) as u16;
                let key_value = (idx % NUM_KEY_STATES) as i32;
                let key_name = get_key_name(key_code);
                let value_name = get_value_name(key_value);

                // Calculate min/avg/max for near-miss timings
                let min_us = timings.iter().min().copied().unwrap_or(0);
                let max_us = timings.iter().max().copied().unwrap_or(0);
                let sum: u64 = timings.iter().sum();
                let avg_us = if !timings.is_empty() {
                    sum / timings.len() as u64
                } else {
                    0
                }; // Integer average is fine here

                near_miss_json_vec.push(NearMissJson {
                    key_code,
                    key_value,
                    key_name,
                    value_name,
                    count: timings.len(),
                    timings_us: timings, // Reference the original timings vector
                    min_us,
                    avg_us,
                    max_us,
                });
            }
        }

        #[derive(Serialize)]
        struct ReportData<'a> {
            report_type: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            runtime_us: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            runtime_human: Option<String>,
            // Add human-readable config values to JSON output
            // Add raw config values as well for machine readability
            debounce_time_us: u64,
            near_miss_threshold_us: u64,
            log_interval_us: u64,
            debounce_time_human: String,
            near_miss_threshold_human: String,
            log_interval_human: String,
            key_events_processed: u64,
            key_events_passed: u64,
            key_events_dropped: u64,
            // Use the new Vec types for serialization
            per_key_stats: Vec<PerKeyStatsJson>, // Removed lifetime 'a
            per_key_passed_near_miss_timing: Vec<NearMissJson<'a>>, // Keep lifetime 'a here for timings_us ref
        }

        let runtime_human = runtime_us.map(|us| util::format_duration(Duration::from_micros(us)));
        let debounce_human = util::format_duration(config.debounce_time());
        let near_miss_human = util::format_duration(config.near_miss_threshold());
        let log_interval_human = util::format_duration(config.log_interval());

        let report = ReportData {
            report_type,
            runtime_us, // Will be None for periodic reports
            runtime_human,
            debounce_time_us: config.debounce_us(), // Add raw value
            near_miss_threshold_us: config.near_miss_threshold_us(), // Add raw value
            log_interval_us: config.log_interval_us(), // Add raw value
            debounce_time_human: debounce_human,
            near_miss_threshold_human: near_miss_human,
            log_interval_human,
            key_events_processed: self.key_events_processed,
            key_events_passed: self.key_events_passed,
            key_events_dropped: self.key_events_dropped,
            per_key_stats: per_key_stats_json_vec, // Use the prepared Vec
            per_key_passed_near_miss_timing: near_miss_json_vec, // Use the prepared Vec
        };

        // We are printing individual reports (cumulative or periodic) as separate JSON objects
        // to stderr. The logger thread handles the overall structure (e.g., a list of periodic
        // reports).
        let _ = serde_json::to_writer_pretty(&mut writer, &report);
        let _ = writeln!(writer);
    }
}

