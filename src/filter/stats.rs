// This module defines the StatsCollector struct and related types
// used by the logger thread to accumulate and report statistics.
use crate::filter::{FILTER_MAP_SIZE, NUM_KEY_STATES};

use crate::filter::keynames::{get_key_name, get_value_name};
use crate::logger::EventInfo;
use crate::util;
use serde::Serialize;
use std::io::Write;
use std::time::Duration;

// Define histogram bucket boundaries in milliseconds.
// These represent the *upper bounds* of the buckets.
// Example: [1, 2, 4] means buckets are <1ms, 1-2ms, 2-4ms, >=4ms.
pub const HISTOGRAM_BUCKET_BOUNDARIES_MS: &[u64] = &[1, 2, 4, 8, 16, 32, 64, 128];
pub const NUM_HISTOGRAM_BUCKETS: usize = HISTOGRAM_BUCKET_BOUNDARIES_MS.len() + 1;

/// Represents a histogram of timing values.
#[derive(Debug, Serialize, Clone)]
pub struct TimingHistogram {
    // Counts per bucket. Index 0 is for values < boundary[0], index N is for values >= boundary[N-1].
    pub buckets: [u64; NUM_HISTOGRAM_BUCKETS],
    // Total count of events recorded in this histogram.
    pub count: u64,
    // Sum of all timings recorded (in microseconds) for calculating average.
    pub sum_us: u64,
    // Optional: Store min/max directly if needed, otherwise calculate from raw data if kept.
    // pub min_us: u64,
    // pub max_us: u64,
}

impl Default for TimingHistogram {
    fn default() -> Self {
        Self {
            buckets: [0; NUM_HISTOGRAM_BUCKETS],
            count: 0,
            sum_us: 0,
        }
    }
}

impl TimingHistogram {
    /// Records a timing value (in microseconds) into the correct bucket.
    #[inline]
    pub fn record(&mut self, timing_us: u64) {
        let timing_ms = timing_us / 1000; // Convert to ms for bucket comparison
        let mut bucket_index = NUM_HISTOGRAM_BUCKETS - 1; // Default to the last bucket (>= last boundary)

        for (i, &boundary_ms) in HISTOGRAM_BUCKET_BOUNDARIES_MS.iter().enumerate() {
            if timing_ms < boundary_ms {
                bucket_index = i;
                break;
            }
        }

        self.buckets[bucket_index] += 1;
        self.count += 1;
        self.sum_us = self.sum_us.saturating_add(timing_us); // Use saturating_add
                                                             // Optional: Update min/max
                                                             // self.min_us = self.min_us.min(timing_us);
                                                             // self.max_us = self.max_us.max(timing_us);
    }

    /// Calculates the average timing in microseconds. Returns 0 if count is 0.
    pub fn average_us(&self) -> u64 {
        if self.count > 0 {
            self.sum_us / self.count
        } else {
            0
        }
    }

    // Add methods like get_buckets(), get_count() if needed externally.
}

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
    /// Count of events that were dropped (bounced) for this specific key state.
    pub dropped_count: u64,
    // Stores the microsecond difference between a dropped event and the previous passed event.
    // Keeping raw timings for now, alongside histogram.
    pub timings_us: Vec<u64>,
    /// Histogram of bounce timings for this specific key state.
    pub bounce_histogram: TimingHistogram,
}

impl KeyValueStats {
    /// Adds a timing value to the vector and records it in the histogram.
    #[inline]
    pub fn push_timing(&mut self, value: u64) {
        if self.timings_us.len() == self.timings_us.capacity() {
            self.timings_us.reserve(1); // Or a larger number
        }
        self.timings_us.push(value);
        self.bounce_histogram.record(value); // Record in histogram
    }
}

/// Statistics for passed events that were near misses for a specific key value state.
#[derive(Debug, Serialize, Clone, Default)]
pub struct NearMissStats {
    // Keep raw timings for now (useful for potential future percentile calc or detailed analysis)
    pub timings_us: Vec<u64>,
    pub histogram: TimingHistogram,
}

impl NearMissStats {
    /// Adds a timing value, resizing if necessary, and records it in the histogram.
    #[inline]
    pub fn push_timing(&mut self, value: u64) {
        if self.timings_us.len() == self.timings_us.capacity() {
            self.timings_us.reserve(1); // Or a larger number
        }
        self.timings_us.push(value);
        self.histogram.record(value); // Record in histogram as well
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
struct PerKeyStatsJson<'this> {
    key_code: u16,
    key_name: &'static str,
    total_processed: u64,
    total_dropped: u64,
    drop_percentage: f64,
    stats: KeyStatsJson<'this>, // Use the new struct holding detailed stats
}

/// Structure for serializing detailed key value stats in JSON.
#[derive(Serialize, Debug)]
struct KeyValueStatsJson<'this> {
    total_processed: u64,
    passed_count: u64,
    dropped_count: u64,
    drop_rate: f64,
    timings_us: &'this Vec<u64>, // Reference original timings
    bounce_histogram: TimingHistogramJson,
}

/// Structure for serializing detailed key stats in JSON.
#[derive(Serialize, Debug)]
struct KeyStatsJson<'this> {
    press: KeyValueStatsJson<'this>,
    release: KeyValueStatsJson<'this>,
    repeat: KeyValueStatsJson<'this>, // Keep repeat for structure consistency
}

/// Structure for serializing histogram data in JSON.
#[derive(Serialize, Debug)]
struct TimingHistogramJson {
    buckets: Vec<HistogramBucketJson>,
    count: u64,
    avg_us: u64,
    // min_us: u64, // Optional
    // max_us: u64, // Optional
}

/// Structure for serializing a single histogram bucket in JSON.
#[derive(Serialize, Debug)]
struct HistogramBucketJson {
    min_ms: u64,
    max_ms: Option<u64>, // None for the last bucket (>= max boundary)
    count: u64,
}

/// Structure for serializing near-miss statistics in JSON.
#[derive(Serialize, Debug)]
struct NearMissStatsJson<'this> {
    key_code: u16,
    key_value: i32,
    key_name: &'static str,
    value_name: &'static str,
    count: usize,
    timings_us: &'this Vec<u64>, // Reference the original timings vector
    near_miss_histogram: TimingHistogramJson,
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
    /// Holds near-miss stats per key code and value. Indexed by `keycode * 3 + value`.
    pub per_key_near_miss_stats: Box<[NearMissStats; FILTER_MAP_SIZE * NUM_KEY_STATES]>,
    /// Overall histogram for all bounce timings. Aggregated before reporting.
    pub overall_bounce_histogram: TimingHistogram,
    /// Overall histogram for all near_miss timings. Aggregated before reporting.
    pub overall_near_miss_histogram: TimingHistogram,
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
        let per_key_near_miss_stats =
            Box::new([(); FILTER_MAP_SIZE * NUM_KEY_STATES].map(|_| NearMissStats::default()));

        StatsCollector {
            key_events_processed: 0,
            key_events_passed: 0,
            key_events_dropped: 0,
            per_key_stats,
            per_key_near_miss_stats,
            overall_bounce_histogram: TimingHistogram::default(),
            overall_near_miss_histogram: TimingHistogram::default(),
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

        // Check bounds before accessing arrays
        if key_code_idx >= FILTER_MAP_SIZE || key_value_idx >= NUM_KEY_STATES {
            // Out of bounds - ignore for stats accumulation
            return;
        }

        let value_stats = match info.event.value {
            1 => &mut self.per_key_stats[key_code_idx].press,
            0 => &mut self.per_key_stats[key_code_idx].release,
            _ => &mut self.per_key_stats[key_code_idx].repeat,
        };

        // Increment total processed count
        value_stats.total_processed += 1;

        // Handle bounce/pass logic
        if info.is_bounce {
            self.key_events_dropped += 1;
            // Increment drop count and record timing
            value_stats.dropped_count += 1; // Increment drop count for this state
            if let Some(diff) = info.diff_us {
                value_stats.push_timing(diff); // Records in Vec and Histogram
            }
        } else {
            // Event passed the filter.
            self.key_events_passed += 1;
            // Increment passed count
            value_stats.passed_count += 1;

            // Check for near-miss on passed events
            if let Some(last_us) = info.last_passed_us {
                if let Some(diff) = info.event_us.checked_sub(last_us) {
                    // Check if the difference is within the near-miss window (debounce_time <= diff <= threshold)
                    // The filter ensures diff >= debounce_time for passed events.
                    // Here, we check against the near_miss threshold.
                    if diff <= config.near_miss_threshold_us() {
                        // Calculate the flat index for the per_key_near_miss_stats array.
                        let idx = key_code_idx * NUM_KEY_STATES + key_value_idx;
                        // Bounds check is already done at the start of the function
                        self.per_key_near_miss_stats[idx].push_timing(diff); // Records in Vec and Histogram
                    }
                }
            }
        }
    }

    /// Aggregates per-key histograms into the overall histograms.
    /// Should be called before generating reports.
    pub fn aggregate_histograms(&mut self) {
        // Reset overall histograms (important if called multiple times, e.g., periodic)
        self.overall_bounce_histogram = TimingHistogram::default();
        self.overall_near_miss_histogram = TimingHistogram::default();

        for key_stats in self.per_key_stats.iter() {
            // Aggregate bounce histograms
            Self::accumulate_histogram(
                &mut self.overall_bounce_histogram,
                &key_stats.press.bounce_histogram,
            );
            Self::accumulate_histogram(
                &mut self.overall_bounce_histogram,
                &key_stats.release.bounce_histogram,
            );
            // Ignore repeat histogram for bounces (repeat events are not debounced)
        }

        for near_miss_stats in self.per_key_near_miss_stats.iter() {
            // Aggregate near_miss histograms
            Self::accumulate_histogram(
                &mut self.overall_near_miss_histogram,
                &near_miss_stats.histogram,
            );
        }
    }

    /// Helper to add counts from a source histogram to a destination histogram.
    #[inline]
    fn accumulate_histogram(dest: &mut TimingHistogram, source: &TimingHistogram) {
        if source.count > 0 {
            dest.count += source.count;
            dest.sum_us = dest.sum_us.saturating_add(source.sum_us);
            for i in 0..NUM_HISTOGRAM_BUCKETS {
                dest.buckets[i] += source.buckets[i];
            }
            // Optional: Update overall min/max if stored directly
            // dest.min_us = dest.min_us.min(source.min_us);
            // dest.max_us = dest.max_us.max(source.max_us);
        }
    }

    /// Formats a `TimingHistogram` into a human-readable string representation.
    fn format_histogram_human(histogram: &TimingHistogram) -> String {
        if histogram.count == 0 {
            return "No data".to_string();
        }

        let mut output = String::new();
        let total_count = histogram.count;

        // Determine max bucket count for scaling the bar
        let max_bucket_count = histogram.buckets.iter().copied().max().unwrap_or(0);
        let bar_scale = if max_bucket_count > 0 {
            50.0 / max_bucket_count as f64
        } else {
            0.0
        }; // Max bar width 50 chars

        for i in 0..NUM_HISTOGRAM_BUCKETS {
            let bucket_count = histogram.buckets[i];
            let percentage = if total_count > 0 {
                (bucket_count as f64 / total_count as f64) * 100.0
            } else {
                0.0
            };

            let label = if i == 0 {
                format!("< {}ms", HISTOGRAM_BUCKET_BOUNDARIES_MS[0])
            } else if i == NUM_HISTOGRAM_BUCKETS - 1 {
                format!(
                    ">= {}ms",
                    HISTOGRAM_BUCKET_BOUNDARIES_MS[NUM_HISTOGRAM_BUCKETS - 2]
                )
            } else {
                format!(
                    "{}-{}ms",
                    HISTOGRAM_BUCKET_BOUNDARIES_MS[i - 1],
                    HISTOGRAM_BUCKET_BOUNDARIES_MS[i]
                )
            };

            let bar_width = (bucket_count as f64 * bar_scale).round() as usize;
            let bar = "#".repeat(bar_width);

            output.push_str(&format!(
                "  {label:<10}: {bucket_count:<5} ({percentage:>5.1}%) [{bar}]\n"
            ));
        }

        let avg_us = histogram.average_us();
        output.push_str(&format!(
            "  Total: {}, Avg: {}\n",
            total_count,
            util::format_us(avg_us)
        ));

        output
    }

    /// Formats human-readable statistics summary and writes it to the provided writer.
    /// Returns an io::Result to handle potential write errors.
    pub fn format_stats_human_readable(
        &mut self, // Needs to be mutable to aggregate histograms
        config: &crate::config::Config,
        report_type: &str,
        mut writer: impl Write, // Accept a generic writer
    ) -> std::io::Result<()> {
        // Aggregate histograms before reporting
        self.aggregate_histograms();

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

        // Overall Bounce Histogram
        writeln!(writer, "\n--- Overall Bounce Timing Histogram ---")?;
        write!(
            writer,
            "{}",
            Self::format_histogram_human(&self.overall_bounce_histogram)
        )?;

        // Overall Near-Miss Histogram
        writeln!(
            writer,
            "\n--- Overall Near-Miss Timing Histogram (Passed within {}) ---",
            util::format_duration(config.near_miss_threshold())
        )?;
        write!(
            writer,
            "{}",
            Self::format_histogram_human(&self.overall_near_miss_histogram)
        )?;

        let mut any_drops = false;
        for key_code in 0..self.per_key_stats.len() {
            let stats = &self.per_key_stats[key_code];
            let total_drops_for_key = stats.press.dropped_count
                + stats.release.dropped_count
                + stats.repeat.dropped_count;

            if total_drops_for_key > 0
                || stats.press.total_processed > 0
                || stats.release.total_processed > 0
                || stats.repeat.total_processed > 0
            {
                // Only print key if it had any activity (passed or dropped)
                if !any_drops {
                    writeln!(writer, "\n--- Dropped Event Statistics Per Key ---")?;
                    writeln!(writer, "Format: Key [Name] (Code):")?;
                    writeln!(
                        writer,
                        "  State (Value): Processed: <count>, Passed: <count>, Dropped: <count> (<rate>%) (Bounce Time: Min / Avg / Max)"
                    )?;
                    any_drops = true;
                }

                let key_name = get_key_name(key_code as u16);
                writeln!(writer, "\nKey [{key_name}] ({key_code}):")?;
                // Calculate total processed for this key
                let total_processed_for_key = stats.press.total_processed
                    + stats.release.total_processed
                    + stats.repeat.total_processed;
                // Calculate total passed for this key
                let total_passed_for_key = stats.press.passed_count
                    + stats.release.passed_count
                    + stats.repeat.passed_count;
                // Calculate overall drop percentage for this key
                let key_drop_percentage = if total_processed_for_key > 0 {
                    // Base percentage on total processed
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
                    if value_stats.total_processed > 0 {
                        // Print if any events (passed or dropped) for this state
                        // Calculate drop rate for this specific state
                        let drop_rate = if value_stats.total_processed > 0 {
                            (value_stats.dropped_count as f64 / value_stats.total_processed as f64)
                                * 100.0
                        } else {
                            0.0
                        };
                        // Updated detail line format
                        write!(
                            writer, // Use write! not writeln!
                            "  {:<7} ({}): Processed: {}, Passed: {}, Dropped: {} ({:.2}%)",
                            value_name,
                            value_code,
                            value_stats.total_processed,
                            value_stats.passed_count,
                            value_stats.dropped_count,
                            drop_rate
                        )?;
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
                            writeln!(writer)?; // Newline if no timing data
                        }
                    }
                    Ok(()) // Return Ok from the closure
                };

                print_value_stats("Press", 1, &stats.press)?;
                print_value_stats("Release", 0, &stats.release)?;
                print_value_stats("Repeat", 2, &stats.repeat)?; // Include repeat stats line if processed
            }
        }
        if !any_drops {
            writeln!(writer, "\n--- No key events dropped ---")?;
        }

        let mut any_near_miss = false;
        for idx in 0..self.per_key_near_miss_stats.len() {
            let near_miss_stats = &self.per_key_near_miss_stats[idx];
            if !near_miss_stats.timings_us.is_empty() {
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
                // Removed unused variable declaration
                // let value_name = get_value_name(key_value);

                let timings = &near_miss_stats.timings_us;
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
    pub fn print_stats_to_stderr(&mut self, config: &crate::config::Config, report_type: &str) {
        // Ignore potential write errors when writing to stderr, as there's not much we can do.
        let _ =
            self.format_stats_human_readable(config, report_type, &mut std::io::stderr().lock());
    }

    /// Helper to create JSON representation of a TimingHistogram.
    fn create_histogram_json(histogram: &TimingHistogram) -> TimingHistogramJson {
        let mut buckets_json = Vec::with_capacity(NUM_HISTOGRAM_BUCKETS);
        for i in 0..NUM_HISTOGRAM_BUCKETS {
            let min_ms = if i == 0 {
                0
            } else {
                HISTOGRAM_BUCKET_BOUNDARIES_MS[i - 1]
            };
            let max_ms = if i == NUM_HISTOGRAM_BUCKETS - 1 {
                None
            } else {
                Some(HISTOGRAM_BUCKET_BOUNDARIES_MS[i])
            };
            buckets_json.push(HistogramBucketJson {
                min_ms,
                max_ms,
                count: histogram.buckets[i],
            });
        }
        TimingHistogramJson {
            buckets: buckets_json,
            count: histogram.count,
            avg_us: histogram.average_us(),
            // min_us: histogram.min_us, // Optional
            // max_us: histogram.max_us, // Optional
        }
    }

    /// Prints statistics in JSON format to the given writer.
    /// Includes runtime provided externally (calculated in main thread).
    pub fn print_stats_json<'this>(
        // Renamed lifetime to 'this
        &'this mut self, // Explicitly tie lifetime to &mut self
        config: &crate::config::Config,
        runtime_us: Option<u64>,
        report_type: &'this str, // report_type also needs this lifetime
        mut writer: impl Write,
    ) {
        // Aggregate histograms before reporting
        self.aggregate_histograms();

        // --- Prepare Per-Key Drop Stats for JSON ---
        let mut per_key_stats_json_vec = Vec::new();
        for (key_code_usize, stats) in self.per_key_stats.iter().enumerate() {
            let total_processed_for_key = stats.press.total_processed
                + stats.release.total_processed
                + stats.repeat.total_processed;
            let total_dropped_for_key = stats.press.dropped_count
                + stats.release.dropped_count
                + stats.repeat.dropped_count;

            if total_processed_for_key > 0 {
                // Include keys with any activity (passed or dropped)
                let key_code = key_code_usize as u16;
                let key_name = get_key_name(key_code);
                let drop_percentage = if total_processed_for_key > 0 {
                    (total_dropped_for_key as f64 / total_processed_for_key as f64) * 100.0
                } else {
                    0.0
                };

                // Helper closure to create KeyValueStatsJson
                let create_kv_stats_json =
                    |kv_stats: &'this KeyValueStats| -> KeyValueStatsJson<'this> {
                        let drop_rate = if kv_stats.total_processed > 0 {
                            (kv_stats.dropped_count as f64 / kv_stats.total_processed as f64)
                                * 100.0
                        } else {
                            0.0
                        };
                        KeyValueStatsJson {
                            total_processed: kv_stats.total_processed,
                            passed_count: kv_stats.passed_count,
                            dropped_count: kv_stats.dropped_count,
                            drop_rate,
                            timings_us: &kv_stats.timings_us,
                            bounce_histogram: Self::create_histogram_json(
                                &kv_stats.bounce_histogram,
                            ),
                        }
                    };

                // Populate the detailed stats structure for JSON
                let detailed_stats_json = KeyStatsJson {
                    // Add lifetime here
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
                    stats: detailed_stats_json, // Use the new detailed struct // Add lifetime here
                });
            }
        }

        // --- Prepare Near-Miss Stats for JSON ---
        let mut near_miss_json_vec = Vec::new();
        for (idx, near_miss_stats) in self.per_key_near_miss_stats.iter().enumerate() {
            if !near_miss_stats.timings_us.is_empty() {
                let key_code = (idx / NUM_KEY_STATES) as u16;
                let key_value = (idx % NUM_KEY_STATES) as i32;
                let key_name = get_key_name(key_code);
                let value_name = get_value_name(key_value);

                near_miss_json_vec.push(NearMissStatsJson {
                    key_code,
                    key_value,
                    key_name,
                    value_name,
                    count: near_miss_stats.timings_us.len(),
                    timings_us: &near_miss_stats.timings_us, // Reference the original timings vector
                    near_miss_histogram: Self::create_histogram_json(&near_miss_stats.histogram),
                });
            }
        }

        #[derive(Serialize)]
        struct ReportData<'this> {
            // Renamed lifetime to 'this
            report_type: &'this str,
            #[serde(skip_serializing_if = "Option::is_none")]
            runtime_us: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            runtime_human: Option<String>,
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
            // Overall Histograms
            overall_bounce_histogram: TimingHistogramJson,
            overall_near_miss_histogram: TimingHistogramJson,
            // Per-Key and Per-Near-Miss details
            per_key_stats: Vec<PerKeyStatsJson<'this>>,
            per_key_near_miss_stats: Vec<NearMissStatsJson<'this>>,
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
            overall_bounce_histogram: Self::create_histogram_json(&self.overall_bounce_histogram),
            overall_near_miss_histogram: Self::create_histogram_json(
                &self.overall_near_miss_histogram,
            ),
            per_key_stats: per_key_stats_json_vec, // Use the prepared Vec
            per_key_near_miss_stats: near_miss_json_vec, // Use the prepared Vec
        };

        // We are printing individual reports (cumulative or periodic) as separate JSON objects
        // to stderr. The logger thread handles the overall structure (e.g., a list of periodic
        // reports).
        let _ = serde_json::to_writer_pretty(&mut writer, &report);
        let _ = writeln!(writer);
    }
}
