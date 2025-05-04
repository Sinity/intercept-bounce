// This module defines the Logger thread, which handles logging events
// and accumulating/reporting statistics based on messages received
// from the main processing thread.

use crate::config::Config;
use crate::event;
use crate::filter::keynames::{get_event_type_name, get_key_name};
use crate::filter::stats::StatsCollector;
use crate::util; // Import util
use crossbeam_channel::{Receiver, RecvTimeoutError}; // Use directly

use chrono::Local;
use input_linux_sys::{input_event, EV_MSC, EV_SYN};
use std::io;
// Removed unused: use std::ops::Deref;
use opentelemetry::metrics::{Counter, Meter}; // Import OTLP metrics types
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;
use tracing::{instrument, Span}; // Import instrument and Span // Only info is used directly in this file's functions

/// Represents a message sent from the main thread to the logger thread.
// #[derive(Clone)] // Remove Clone
// #[derive(Debug)] // input_event does not implement Debug
pub enum LogMessage {
    /// Contains detailed information about a single processed event.
    Event(EventInfo),
}

/// Detailed information about a single processed event, sent to the logger.
// #[derive(Clone)] // Remove Clone
// #[derive(Debug)] // input_event does not implement Debug
pub struct EventInfo {
    /// The raw input event.
    pub event: input_event,
    /// Timestamp of the event in microseconds.
    pub event_us: u64,
    /// Result of the bounce check (`true` if bounced/dropped).
    pub is_bounce: bool,
    /// Time difference (µs) between this event and the previous passed event
    /// of the same type, *only if* `is_bounce` is true.
    pub diff_us: Option<u64>,
    /// Timestamp (µs) of the previous event of the same type that *passed* the filter.
    /// This is needed by the logger thread to calculate near-miss statistics.
    pub last_passed_us: Option<u64>,
}

/// Manages the state and execution loop for the logger thread.
pub struct Logger {
    receiver: Receiver<LogMessage>, // Use Receiver directly
    logger_running: Arc<AtomicBool>,
    config: Arc<Config>,

    cumulative_stats: StatsCollector,
    interval_stats: StatsCollector,

    last_dump_time: Instant,
    first_event_us: Option<u64>,

    // Optional OTLP Meter for logger-specific metrics
    otel_meter: Option<Meter>,
}

impl Logger {
    /// Creates a new Logger instance.
    pub fn new(
        receiver: Receiver<LogMessage>, // Use Receiver directly
        logger_running: Arc<AtomicBool>,
        config: Arc<Config>,
        otel_meter: Option<Meter>, // Accept optional meter
    ) -> Self {
        Logger {
            receiver, // Use Receiver directly
            logger_running,
            config,
            cumulative_stats: StatsCollector::with_capacity(),
            interval_stats: StatsCollector::with_capacity(),
            last_dump_time: Instant::now(),
            first_event_us: None,
            otel_meter, // Store the meter
        }
    }

    /// Manages the logger thread's main loop.
    ///
    /// It receives messages from the main thread, processes them (logging and stats),
    /// and handles periodic stats dumping. It exits when the `logger_running` flag
    /// is set to false and the channel is empty or disconnected.
    ///
    /// Returns the final cumulative statistics upon exit.
    pub fn run(&mut self) -> StatsCollector {
        // Use tracing for logger thread startup message
        tracing::debug!("Logger thread started");
        let log_interval = self.config.log_interval(); // Get Duration directly
        let check_interval = Duration::from_millis(100); // Used for periodic checks

        // --- OTLP Metrics Setup (in logger thread) ---
        let near_miss_counter: Option<Counter<u64>> = self.otel_meter.as_ref().map(|m| {
            m.u64_counter("events.near_miss")
                .with_description("Passed events that were near misses")
                .init()
        });

        loop {
            // Check running flag first
            if !self.logger_running.load(Ordering::SeqCst) {
                tracing::debug!(
                    "Received shutdown signal via AtomicBool, attempting to drain channel"
                );
                while let Ok(msg) = self.receiver.try_recv() {
                    tracing::trace!("Draining channel: Processing message after shutdown signal");
                    self.process_message(msg, &near_miss_counter); // Pass counter
                }
                tracing::debug!("Finished draining channel. Exiting run loop");
                break;
            }

            // Check periodic stats dump timer
            if log_interval > Duration::ZERO && self.last_dump_time.elapsed() >= log_interval {
                tracing::debug!("Triggering periodic stats dump");
                self.dump_periodic_stats();
                self.last_dump_time = Instant::now();
                tracing::debug!("Periodic stats dump complete. Timer reset");
            }

            // Receive messages with timeout
            match self.receiver.recv_timeout(check_interval) {
                Ok(msg) => {
                    tracing::trace!("Logger thread received message from channel");
                    self.process_message(msg, &near_miss_counter); // Pass counter
                    tracing::trace!("Logger thread finished processing message");
                }
                Err(RecvTimeoutError::Timeout) => {
                    // No message received, loop continues to check flags/timer
                    tracing::trace!("Logger thread receive timed out. Re-checking flags");
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    tracing::warn!("Detected channel disconnected. Attempting to drain channel");
                    while let Ok(msg) = self.receiver.try_recv() {
                        tracing::trace!(
                            "Logger thread draining channel: Processing message after disconnect"
                        );
                        self.process_message(msg, &near_miss_counter); // Pass counter
                    }
                    tracing::warn!("Finished draining channel. Exiting run loop");
                    break; // Exit loop on disconnect
                }
            }
        } // End loop

        tracing::debug!("Run loop exited. Preparing final stats");
        tracing::debug!("Taking cumulative_stats for return");
        std::mem::take(&mut self.cumulative_stats)
    }

    /// Processes a single message received from the main thread.
    /// Updates statistics and performs logging if enabled.
    #[instrument(name = "logger_process_message", skip(self, msg, near_miss_counter), fields(otel.kind = "internal", event_type=tracing::field::Empty, is_bounce=tracing::field::Empty))]
    pub fn process_message(
        &mut self,
        msg: LogMessage,
        near_miss_counter: &Option<Counter<u64>>, // Receive counter
    ) {
        // Made public for benches/tests
        match msg {
            LogMessage::Event(data) => {
                // Log EventInfo fields individually at trace level
                tracing::trace!(event_type = data.event.type_,
                       event_code = data.event.code,
                       event_value = data.event.value,
                       event_us = data.event_us,
                       is_bounce = data.is_bounce,
                       diff_us = ?data.diff_us,
                       last_passed_us = ?data.last_passed_us,
                       "Logger processing EventInfo");
                // Record event details in the current span
                Span::current().record("event_type", data.event.type_);
                Span::current().record("is_bounce", data.is_bounce);

                self.cumulative_stats
                    .record_event_info_with_config(&data, &self.config);
                self.interval_stats
                    .record_event_info_with_config(&data, &self.config);

                if self.first_event_us.is_none() {
                    self.first_event_us = Some(data.event_us);
                    tracing::trace!(ts = data.event_us, "Logger recorded first event timestamp");
                }

                // --- Increment Near-Miss Counter ---
                if !data.is_bounce && event::is_key_event(&data.event) {
                    if let Some(last_us) = data.last_passed_us {
                        if let Some(diff) = data.event_us.checked_sub(last_us) {
                            if diff <= self.config.near_miss_threshold_us() {
                                if let Some(counter) = near_miss_counter {
                                    counter.add(1, &[]);
                                }
                            }
                        }
                    }
                }

                if self.config.log_all_events {
                    if data.event.type_ == EV_SYN as u16 || data.event.type_ == EV_MSC as u16 {
                        return; // Skip logging SYN/MSC events even in log-all mode
                    }
                    tracing::trace!("Logger logging all events");
                    self.log_event_detailed(&data);
                } else if self.config.log_bounces
                    && data.is_bounce
                    && event::is_key_event(&data.event)
                {
                    tracing::trace!("Logger logging bounce event");
                    self.log_simple_bounce_detailed(&data);
                }
            }
        }
    }

    /// Dumps the current interval statistics to stderr.
    #[instrument(name = "dump_periodic_stats", skip(self))]
    fn dump_periodic_stats(&mut self) {
        let wallclock = Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        tracing::info!(target: "stats", kind = "periodic", wallclock = %wallclock, "Periodic stats dump");

        let interval_stats_clone = self.interval_stats.clone();
        if self.config.stats_json {
            tracing::debug!("Logger thread printing periodic stats in JSON format");
            interval_stats_clone.print_stats_json(
                &self.config, // Remove explicit auto-deref
                None,                     // Runtime is only for cumulative
                "Periodic",               // Report type
                &mut io::stderr().lock(), // Write directly to stderr
            );
            tracing::debug!("Logger thread finished printing periodic stats in JSON format");
        } else {
            tracing::debug!("Logger thread printing periodic stats in human-readable format");
            interval_stats_clone.print_stats_to_stderr(&self.config, "Periodic"); // Remove explicit auto-deref
            tracing::debug!(
                "Logger thread finished printing periodic stats in human-readable format"
            );
        }

        tracing::debug!("Logger thread resetting interval stats");
        self.interval_stats = StatsCollector::with_capacity();
        tracing::debug!("Logger thread interval stats reset");
    }

    /// Adapts logic from the old BounceFilter::log_event.
    /// Logs details of a single event (passed or dropped) using tracing.
    #[instrument(name = "log_event_detailed", skip(self, data), fields(otel.kind = "internal", status=tracing::field::Empty, key_code=data.event.code))]
    fn log_event_detailed(&self, data: &EventInfo) {
        let status = if data.is_bounce { "DROP" } else { "PASS" };

        // Use saturating_sub for manual_saturating_arithmetic
        let relative_us = data
            .event_us
            .saturating_sub(self.first_event_us.unwrap_or(data.event_us));

        let type_name = get_event_type_name(data.event.type_);

        let (key_name_str, value_name_str) = if event::is_key_event(&data.event) {
            let key_name = get_key_name(data.event.code);
            let value_name = match data.event.value {
                0 => "Release",
                1 => "Press",
                2 => "Repeat",
                _ => "Unknown",
            };
            (key_name, value_name)
        } else {
            ("", "") // Not a key event, no key/value names
        };

        let bounce_info_str = if data.is_bounce && event::is_key_event(&data.event) {
            if let Some(diff) = data.diff_us {
                format!(" (Bounce Time: {})", util::format_us(diff))
            } else {
                " (Bounce Time: N/A)".to_string()
            }
        } else {
            "".to_string()
        };

        let near_miss_info_str = if !data.is_bounce && event::is_key_event(&data.event) {
            if let Some(last_us) = data.last_passed_us {
                if let Some(diff) = data.event_us.checked_sub(last_us) {
                    // Check against Duration directly, use accessor
                    if Duration::from_micros(diff) >= self.config.debounce_time()
                        && Duration::from_micros(diff) <= self.config.near_miss_threshold()
                    {
                        format!(" (Diff since last passed: {})", util::format_us(diff))
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            }
        } else {
            "".to_string()
        };

        // Use info! macro for event logging
        info!(
            // target: "event", // Use a specific target for event logs - REMOVED for test compatibility
            status = status,
            relative_us = relative_us,
            relative_human = %format_relative_us(relative_us), // Include formatted string
            event_type = data.event.type_,
            event_type_name = type_name,
            event_code = data.event.code,
            event_value = data.event.value,
            key_name = key_name_str,
            value_name = value_name_str,
            is_bounce = data.is_bounce,
            bounce_time_us = data.diff_us, // Use Option<u64> directly
            bounce_info = %bounce_info_str, // Include formatted string
            near_miss_diff_us = if !data.is_bounce && event::is_key_event(&data.event) { data.event_us.checked_sub(data.last_passed_us.unwrap_or(0)) } else { None }, // Calculate diff for near miss field
            near_miss_info = %near_miss_info_str, // Include formatted string
            "[{}] {} {} ({}, {} {}){}{}{}",
            status,
            format_relative_us(relative_us),
            type_name,
            data.event.code,
            value_name_str,
            data.event.value,
            if event::is_key_event(&data.event) { format!(" Key [{}] ({})", key_name_str, data.event.code) } else { "".to_string() },
            bounce_info_str,
            near_miss_info_str
        );
    }

    /// Adapts logic from the old BounceFilter::log_simple_bounce.
    /// This is used when only `--log-bounces` is enabled. Logs only dropped key events.
    #[instrument(name = "log_simple_bounce_detailed", skip(self, data), fields(otel.kind = "internal", key_code=data.event.code))]
    fn log_simple_bounce_detailed(&self, data: &EventInfo) {
        // This function is only called if data.is_bounce is true and it's a key event.
        let code = data.event.code;
        let value = data.event.value;
        let type_name = get_event_type_name(data.event.type_);
        let key_name = get_key_name(code);

        let value_name = match value {
            0 => "Release",
            1 => "Press",
            2 => "Repeat", // Should not happen based on call site logic, but handle defensively
            _ => "Unknown",
        };

        // Use saturating_sub for manual_saturating_arithmetic
        let relative_us = data
            .event_us
            .saturating_sub(self.first_event_us.unwrap_or(data.event_us));

        let bounce_info_str = if let Some(diff) = data.diff_us {
            format!(" (Bounce Time: {})", util::format_us(diff))
        } else {
            " (Bounce Time: N/A)".to_string()
        };

        // Use info! macro for bounce logging
        info!(
            // target: "event", // Use a specific target for event logs - REMOVED for test compatibility
            status = "DROP",
            relative_us = relative_us,
            relative_human = %format_relative_us(relative_us), // Include formatted string
            event_type = data.event.type_,
            event_type_name = type_name,
            event_code = code,
            event_value = value,
            key_name = key_name,
            value_name = value_name,
            is_bounce = true,
            bounce_time_us = data.diff_us, // Use Option<u64> directly
            bounce_info = %bounce_info_str, // Include formatted string
            "[{}] {} {} ({}, {} {}) Key [{}] ({}){}",
            "DROP",
            format_relative_us(relative_us),
            type_name,
            code,
            value_name,
            value,
            key_name,
            code,
            bounce_info_str
        );
    }
}

/// Helper to format relative timestamps consistently for logging.
fn format_relative_us(relative_us: u64) -> String {
    let s = if relative_us < 1_000 {
        format!("+{relative_us} µs") // Use inline formatting
    } else if relative_us < 1_000_000 {
        format!("+{:.1} ms", relative_us as f64 / 1000.0)
    } else {
        format!("+{:.3} s", relative_us as f64 / 1_000_000.0)
    };
    format!("{s:<10}") // Use inline formatting
}

// format_us moved to src/util.rs
