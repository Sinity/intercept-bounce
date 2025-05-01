// This module defines the StatsCollector struct and related types
// used by the logger thread to accumulate and report statistics.

use crate::event; // Need event::is_key_event
use crate::filter::keynames::get_key_name;
use crate::logger::EventInfo; // Use EventInfo from logger module
use colored::*;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write; // Need Write for print_stats_json

/// Metadata included in JSON statistics output, providing context.
#[derive(Serialize, Clone, Debug)] // Add Clone, Debug
pub struct Meta {
    pub debounce_time_us: u64,
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
    #[inline] // Add inline hint
    pub fn push_timing(&mut self, value: u64) {
        // Use reserve(1) to let the allocator handle growth efficiently
        // when the vector is full. This avoids potentially overallocating
        // if the capacity is often exactly met.
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
    pub key_events_passed: u64,
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
        // Pre-allocate arrays and vectors to minimize reallocations during runtime.
        // The sizes (1024 for key codes, 3072 for key code * value) cover the typical range.
        let per_key_stats = Box::new([(); 1024].map(|_| KeyStats::default()));
        let per_key_passed_near_miss_timing =
            Box::new([(); 3072].map(|_| Vec::with_capacity(1024))); // Pre-allocate inner Vecs
        StatsCollector {
            key_events_processed: 0,
            key_events_passed: 0,
            key_events_dropped: 0,
            per_key_stats,
            per_key_passed_near_miss_timing,
        }
    }

    /// Updates statistics based on information about a processed event.
    /// This is the central method for stats accumulation, called by the logger thread.
    pub fn record_event_info(&mut self, info: &EventInfo) {
        // Only process EV_KEY events for these statistics.
        if !event::is_key_event(&info.event) {
            return;
        }

        self.key_events_processed += 1;

        // Ensure key code and value are within the bounds of our arrays.
        let key_code_idx = info.event.code as usize;
        let key_value_idx = info.event.value as usize; // Used for near-miss index

        if info.is_bounce {
            self.key_events_dropped += 1;
            // Record bounce details if within bounds.
            if key_code_idx < 1024 && key_value_idx < 3 {
                let key_stats = &mut self.per_key_stats[key_code_idx];
                // Determine which KeyValueStats to update (press, release, repeat).
                let value_stats = match info.event.value {
                    1 => &mut key_stats.press,
                    0 => &mut key_stats.release,
                    // Bounces shouldn't happen for repeats, but handle defensively.
                    _ => &mut key_stats.repeat,
                };
                value_stats.count += 1;
                // Record the time difference that caused the bounce.
                if let Some(diff) = info.diff_us {
                    value_stats.push_timing(diff);
                }
            }
        } else {
            // Event passed the filter.
            self.key_events_passed += 1;
            // Check if this passed event qualifies as a "near miss".
            // A near miss is a passed event that occurred shortly after the
            // *previous* passed event of the same type (within 100ms).
            if let Some(last_us) = info.last_passed_us {
                // Ensure the event wasn't actually a bounce (should be guaranteed by is_bounce=false, but check defensively)
                // and calculate the difference.
                if let Some(diff) = info.event_us.checked_sub(last_us) {
                    // Use a fixed threshold (e.g., 100ms) for near misses.
                    const NEAR_MISS_THRESHOLD_US: u64 = 100_000;
                    // Check if the difference is within the near-miss window (debounce_time <= diff < threshold)
                    // We need debounce_time_us here. Pass it into record_event_info?
                    // For now, assume near-miss is just < threshold. The logger can refine this.
                    // TODO: Refine near-miss logic if exact debounce time is needed here.
                    // Let's assume near-miss is simply < 100ms for passed events for now.
                    if diff < NEAR_MISS_THRESHOLD_US {
                        self.record_near_miss((info.event.code, info.event.value), diff);
                    }
                }
                // Ignore cases where time appears to go backwards (checked_sub fails).
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
    // Note: runtime_us is no longer calculated or printed here.
    // It's calculated from BounceFilter state and printed in main.rs.
    pub fn print_stats_to_stderr(
        &self, // Corrected: Only one &self
        debounce_time_us: u64,
        log_all_events: bool,
        log_bounces: bool,
        log_interval_us: u64,
    ) {
        // --- Configuration Summary ---
        // Display the settings the filter was running with.
        eprintln!("{}", "--- intercept-bounce status ---".on_bright_black().bold().blue().underline());
        eprintln!(
            "{} {}", // Use bright_yellow for config values
            "Debounce Threshold:".on_bright_black().bold().bright_white(), // Label color
            format_us(debounce_time_us).on_bright_black().bright_yellow().bold() // Value color
        );
        eprintln!(
            "{} {}", // Use bright_cyan for logging flags
            "Log All Events (--log-all-events):".on_bright_black().bold().bright_white(), // Label color
            if log_all_events { "Active".on_green().black().bold() } else { "Inactive".on_bright_black().dimmed() } // Value color/style
        );
        eprintln!(
            "{} {}", // Use bright_red for log-bounces
            "Log Bounces (--log-bounces):".on_bright_black().bold().bright_white(), // Label color
            if log_all_events {
                "Overridden by --log-all-events".on_bright_black().dimmed() // Dimmed if overridden
            } else if log_bounces {
                "Active".on_red().white().bold() // Keep red if active
            } else {
                "Inactive".on_bright_black().dimmed() // Corrected: Added else block
            }
        );
        eprintln!(
            "{} {}", // Use bright_magenta for interval
            "Periodic Log Interval (--log-interval):".on_bright_black().bold().bright_white(), // Label color
            if log_interval_us > 0 {
                format!("Every {} seconds", log_interval_us / 1_000_000).on_bright_black().bright_magenta().bold() // Value color
            } else {
                "Disabled".on_bright_black().dimmed() // Value style
            }
        );

        // --- Event Counts ---
        eprintln!("\n{}", "--- Overall Statistics ---".on_bright_black().bold().blue().underline());
        eprintln!(
            "{} {}", // Use bright_white for total
            "Key Events Processed:".on_bright_black().bold().bright_white(), // Label color
            self.key_events_processed.to_string().on_bright_black().bright_white().bold() // Value color
        );
        eprintln!(
            "{} {}", // Use bright_green for passed
            "Key Events Passed:   ".on_bright_black().bold().bright_white(), // Label color
            self.key_events_passed.to_string().on_bright_black().bright_green().bold() // Value color
        );
        eprintln!(
            "{} {}", // Use bright_red for dropped
            "Key Events Dropped:  ".on_bright_black().bold().bright_white(), // Label color
            self.key_events_dropped.to_string().on_bright_black().bright_red().bold() // Value color
        );
        let percentage = if self.key_events_processed > 0 { // Calculate percentage dropped
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
        } else {
            0.0 // Avoid division by zero
        };
        eprintln!(
            "{} {:.2}%", // Format percentage
            "Percentage Dropped:  ".on_bright_black().bold().bright_white(), // Label color
            percentage.to_string().on_bright_black().bright_red().bold() // Value color
        );

        // --- Dropped Event Details ---
        let mut any_drops = false;
        // Iterate through the pre-allocated array of per-key stats.
        for key_code in 0..self.per_key_stats.len() {
            let stats = &self.per_key_stats[key_code];
            // Calculate total drops for this key to decide if we print anything.
            let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

            if total_drops_for_key > 0 {
                // Print the header only once when the first dropped key is found.
                if !any_drops {
                    eprintln!("\n{}", "--- Dropped Event Statistics Per Key ---".on_bright_black().bold().blue().underline());
                    eprintln!("{}", "Format: Key [Name] (Code):".on_bright_black().dimmed());
                    eprintln!("{}", "  State (Value): Drop Count (Bounce Time: Min / Avg / Max)".on_bright_black().dimmed());
                    any_drops = true;
                }

                // Print key identifier.
                let key_name = get_key_name(key_code as u16).on_bright_black().bright_magenta().bold();
                eprintln!(
                    "\n{}",
                    format!("Key [{}] ({}):", key_name, key_code).on_bright_black().bold().cyan()
                );

                // Helper closure to print stats for a specific value (press/release/repeat).
                let print_value_stats = |value_name: &str, value_code: i32, value_stats: &KeyValueStats| {
                    if value_stats.count > 0 {
                        // Print state name, value code, and drop count.
                        eprint!(
                            "  {:<7} ({}): {}", // Left-align state name
                            value_name.on_bright_black().bold().bright_white(), // State name color
                            value_code.to_string().on_bright_black().bright_blue().bold(), // Value code color
                            value_stats.count.to_string().on_red().white().bold() // Drop count color
                        );
                        // Calculate and print timing stats if available.
                        if !value_stats.timings_us.is_empty() {
                            let timings = &value_stats.timings_us;
                            // Use checked min/max/sum for robustness, though unlikely to fail.
                            let min = timings.iter().min().copied().unwrap_or(0);
                            let max = timings.iter().max().copied().unwrap_or(0);
                            let sum: u64 = timings.iter().sum();
                            let avg = if !timings.is_empty() { sum as f64 / timings.len() as f64 } else { 0.0 };
                            eprintln!(
                                " ({}: {} / {} / {})",
                                "Bounce Time".on_bright_black().bright_red().bold(), // Label
                                format_us(min).on_bright_black().bright_red().bold(), // Min time
                                format_us(avg as u64).on_bright_black().bright_yellow().bold(), // Avg time
                                format_us(max).on_bright_black().bright_red().bold() // Max time
                            );
                        } else {
                            // Indicate if no timing data was collected (shouldn't happen if count > 0).
                            eprintln!("{}", " (No timing data)".on_bright_black().dimmed());
                        }
                    }
                };

                // Print stats for Press, Release, and Repeat states.
                print_value_stats("Press", 1, &stats.press);
                print_value_stats("Release", 0, &stats.release);
                // Repeat stats are usually 0 as repeats aren't debounced, but print if present.
                print_value_stats("Repeat", 2, &stats.repeat);
            }
        }
        // If no keys had any drops, print a confirmation message.
        if !any_drops {
            eprintln!(
                "\n{}",
                "--- No key events dropped ---".on_bright_black().green().bold()
            );
        }

        // --- Near Miss Details ---
        let mut any_near_miss = false;
        // Iterate through the near-miss timing array.
        for idx in 0..self.per_key_passed_near_miss_timing.len() {
            let timings = &self.per_key_passed_near_miss_timing[idx];
            if !timings.is_empty() {
                 // Print the header only once when the first near miss is found.
                if !any_near_miss {
                    eprintln!(
                        "\n{}",
                        "--- Passed Event Near-Miss Statistics (Passed within 100ms) ---"
                            .on_bright_black()
                            .bold()
                            .blue()
                            .underline()
                    );
                    eprintln!("{}", "Format: Key [Name] (Code, Value): Count (Near-Miss Time: Min / Avg / Max)".on_bright_black().dimmed());
                    any_near_miss = true;
                }

                // Decode key code and value from the flat index.
                let key_code = (idx / 3) as u16;
                let key_value = (idx % 3) as i32;
                let key_name = get_key_name(key_code).on_bright_black().bright_magenta().bold();

                // Calculate timing statistics.
                let min = timings.iter().min().copied().unwrap_or(0);
                let max = timings.iter().max().copied().unwrap_or(0);
                let sum: u64 = timings.iter().sum();
                let avg = if !timings.is_empty() { sum as f64 / timings.len() as f64 } else { 0.0 };

                // Print near-miss details for this key/value combination.
                eprintln!(
                    "  Key [{}] ({}, {}): {} ({}: {} / {} / {})",
                    key_name,
                    key_code.to_string().on_bright_black().bright_blue().bold(), // Key code
                    key_value.to_string().on_bright_black().bright_yellow().bold(), // Key value
                    timings.len().to_string().on_bright_black().bright_white().bold(), // Count
                    "Near-Miss Time".on_bright_black().bright_green().bold(), // Label
                    format_us(min).on_bright_black().bright_green().bold(), // Min time
                    format_us(avg as u64).on_bright_black().bright_yellow().bold(), // Avg time
                    format_us(max).on_bright_black().bright_green().bold() // Max time
                );
            }
        }
         // If no near misses were recorded, print a confirmation message.
        if !any_near_miss {
            eprintln!(
                "\n{}",
                "--- No near-miss events recorded (< 100ms) ---"
                    .on_bright_black()
                    .green()
                    .bold()
            );
        }

        // --- Footer ---
        eprintln!("{}", "----------------------------------------------------------".on_bright_black().blue().bold());
    }

    /// Prints statistics in JSON format to the given writer.
    /// Includes runtime provided externally (calculated in main thread).
    pub fn print_stats_json(
        &self,
        debounce_time_us: u64,
        log_all_events: bool,
        log_bounces: bool,
        log_interval_us: u64,
        runtime_us: Option<u64>, // Runtime is passed in
        mut writer: impl Write, // Use std::io::Write
    ) {
        // --- Data Preparation for Serialization ---
        // To avoid serializing potentially huge arrays with mostly default values,
        // we collect only the keys with non-zero drop counts or non-empty near-miss timings
        // into HashMaps, which Serde can then serialize efficiently.

        // Collect non-default per-key drop statistics.
        let mut per_key_stats_map = HashMap::new();
        for (key_code, stats) in self.per_key_stats.iter().enumerate() {
            // Only include keys that had any drops
            if stats.press.count > 0 || stats.release.count > 0 || stats.repeat.count > 0 {
                // Use key_code as u16 for the map key.
                per_key_stats_map.insert(key_code as u16, stats);
            }
        }

        // Collect non-empty near-miss timing vectors.
        let mut near_miss_map = HashMap::new();
        for (idx, timings) in self.per_key_passed_near_miss_timing.iter().enumerate() {
            if !timings.is_empty() {
                // Decode key code and value from the index.
                let key_code = (idx / 3) as u16;
                let key_value = (idx % 3) as i32;
                // Use (code, value) tuple as the map key.
                near_miss_map.insert((key_code, key_value), timings);
            }
        }

        // --- Define Serialization Structures ---
        // Define temporary structs that borrow the filtered data for serialization.
        // This avoids cloning large amounts of data unnecessarily.

        #[derive(Serialize)]
        struct FilteredStatsData<'a> {
            key_events_processed: u64,
            key_events_passed: u64,
            key_events_dropped: u64,
            // References to the HashMaps containing only non-default data.
            per_key_stats: &'a HashMap<u16, &'a KeyStats>,
            per_key_passed_near_miss_timing: &'a HashMap<(u16, i32), &'a Vec<u64>>,
        }

        // Top-level structure for the final JSON output.
        #[derive(Serialize)]
        struct JsonOutput<'a> {
            meta: Meta,              // Configuration metadata
            runtime_us: Option<u64>, // Overall runtime duration
            stats: FilteredStatsData<'a>, // Aggregated statistics
        }

        // --- Create Instances and Serialize ---
        // Create the stats data structure using references to our filtered maps.
        let filtered_stats_data = FilteredStatsData {
            key_events_processed: self.key_events_processed,
            key_events_passed: self.key_events_passed,
            key_events_dropped: self.key_events_dropped,
            per_key_stats: &per_key_stats_map,
            per_key_passed_near_miss_timing: &near_miss_map,
        };

        // Create the metadata structure.
        let meta = Meta {
            debounce_time_us,
            log_all_events,
            log_bounces,
            log_interval_us,
        };

        // Create the final output structure.
        let output = JsonOutput {
            meta,
            runtime_us, // Use the runtime passed as an argument
            stats: filtered_stats_data,
        };

        // Serialize the output structure to the writer as pretty-printed JSON.
        // Ignore potential errors during serialization for simplicity, though
        // proper error handling might be desired in production code.
        let _ = serde_json::to_writer_pretty(&mut writer, &output);
        // Add a newline after the JSON for better terminal output.
        let _ = writeln!(writer);
    }
}

/// Formats a duration in microseconds into a human-readable string (µs or ms).
#[inline]
pub fn format_us(us: u64) -> String {
    if us >= 1000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{} µs", us)
    }
}
