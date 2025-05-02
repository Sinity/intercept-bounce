// This module defines the Logger thread, which handles logging events
// and accumulating/reporting statistics based on messages received
// from the main processing thread.

use crate::event;
use crate::event;
use crate::filter::keynames::{get_key_name, get_event_type_name};
use crate::filter::stats::StatsCollector;
use crate::config::Config;
use crate::util; // Import util
use crossbeam_channel::Receiver; // Keep for now, replace later
use input_linux_sys::{input_event, EV_SYN, EV_MSC};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use chrono::Local;

/// Represents a message sent from the main thread to the logger thread.
// #[derive(Debug)] // input_event does not implement Debug
pub enum LogMessage {
    /// Contains detailed information about a single processed event.
    Event(EventInfo),
}

/// Detailed information about a single processed event, sent to the logger.
#[derive(Clone)]
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
    receiver: Receiver<LogMessage>,
    logger_running: Arc<AtomicBool>,
    config: Arc<Config>,

    cumulative_stats: StatsCollector,
    interval_stats: StatsCollector,

    last_dump_time: Instant,
    first_event_us: Option<u64>,
}

impl Logger {
    /// Creates a new Logger instance.
    pub fn new(
        receiver: Receiver<LogMessage>,
        logger_running: Arc<AtomicBool>,
        config: Arc<Config>,
    ) -> Self {
        Logger {
            receiver,
            logger_running,
            config,
            cumulative_stats: StatsCollector::with_capacity(),
            interval_stats: StatsCollector::with_capacity(),
            last_dump_time: Instant::now(),
            first_event_us: None,
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
        tracing::debug!("Logger thread started.");
        let log_interval = self.config.log_interval(); // Get Duration directly
        let check_interval = Duration::from_millis(100);

        loop {
            if !self.logger_running.load(Ordering::SeqCst) {
                tracing::debug!("Received shutdown signal via AtomicBool, attempting to drain channel.");
                while let Ok(msg) = self.receiver.try_recv() {
                    tracing::trace!("Draining channel: Processing message after shutdown signal.");
                    self.process_message(msg);
                }
                tracing::debug!("Finished draining channel. Exiting run loop.");
                break;
            }

            if log_interval > Duration::ZERO && self.last_dump_time.elapsed() >= log_interval {
                tracing::debug!("Triggering periodic stats dump.");
                self.dump_periodic_stats();
                self.last_dump_time = Instant::now();
                tracing::debug!("Periodic stats dump complete. Timer reset.");
            }

            match self.receiver.recv_timeout(check_interval) {
                Ok(msg) => {
                    tracing::trace!("Logger thread received message from channel.");
                    self.process_message(msg);
                    tracing::trace!("Logger thread finished processing message.");
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    tracing::trace!("Logger thread receive timed out. Re-checking flags.");
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    tracing::warn!("Detected channel disconnected. Attempting to drain channel.");
                    while let Ok(msg) = self.receiver.try_recv() {
                        tracing::trace!("Logger thread draining channel: Processing message after disconnect.");
                        self.process_message(msg);
                    }
                    tracing::warn!("Finished draining channel. Exiting run loop.");
                    break;
                }
            }
        }

        tracing::debug!("Run loop exited. Preparing final stats.");
        tracing::debug!("Taking cumulative_stats for return.");
        std::mem::take(&mut self.cumulative_stats)
    }

    /// Processes a single message received from the main thread.
    /// Updates statistics and performs logging if enabled.
    pub fn process_message(&mut self, msg: LogMessage) { // Made public for benches/tests
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

                self.cumulative_stats.record_event_info_with_config(&data, &self.config);
                self.interval_stats.record_event_info_with_config(&data, &self.config);

                if self.first_event_us.is_none() {
                    self.first_event_us = Some(data.event_us);
                    tracing::trace!(ts = data.event_us, "Logger recorded first event timestamp");
                }

                if self.config.log_all_events {
                    if data.event.type_ == EV_SYN as u16 || data.event.type_ == EV_MSC as u16 {
                        return; // Skip logging SYN/MSC events even in log-all mode
                    }
                    tracing::trace!("Logger logging all events.");
                    self.log_event_detailed(&data);
                } else if self.config.log_bounces && data.is_bounce && event::is_key_event(&data.event) {
                    tracing::trace!("Logger logging bounce event.");
                    self.log_simple_bounce_detailed(&data);
                }
            }
        }
    }

    /// Dumps the current interval statistics to stderr.
    fn dump_periodic_stats(&mut self) {
        let wallclock = Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        tracing::info!(target: "stats", kind = "periodic", wallclock = %wallclock, "Periodic stats dump");

        let interval_stats_clone = self.interval_stats.clone();
        if self.config.stats_json {
            tracing::debug!("Logger thread printing periodic stats in JSON format.");
            interval_stats_clone.print_stats_json(
                &*self.config,
                None, // Runtime is only for cumulative
                "Periodic", // Report type
                &mut io::stderr().lock(), // Write directly to stderr
            );
            tracing::debug!("Logger thread finished printing periodic stats in JSON format.");
        } else {
            tracing::debug!("Logger thread printing periodic stats in human-readable format.");
            interval_stats_clone.print_stats_to_stderr(&*self.config, "Periodic"); // Pass config and type
            tracing::debug!("Logger thread finished printing periodic stats in human-readable format.");
        }

        tracing::debug!("Logger thread resetting interval stats.");
        self.interval_stats = StatsCollector::with_capacity();
        tracing::debug!("Logger thread interval stats reset.");
    }

    /// Adapts logic from the old BounceFilter::log_event.
    fn log_event_detailed(&self, data: &EventInfo) {
        let status = if data.is_bounce {
            "[DROP]"
        } else {
            "[PASS]"
        };

        let relative_us = data
            .event_us
            .checked_sub(self.first_event_us.unwrap_or(data.event_us))
            .unwrap_or(0);

        let type_name = get_event_type_name(data.event.type_);

        let (key_info, value_name) = if event::is_key_event(&data.event) {
            let key_name = get_key_name(data.event.code);
            let value_name = match data.event.value {
                0 => "Release",
                1 => "Press",
                2 => "Repeat",
                _ => "Unknown",
            };
            (format!(" Key [{}] ({})", key_name, data.event.code), value_name)
        } else {
            ("".to_string(), "")
        };

        let bounce_info = if data.is_bounce && event::is_key_event(&data.event) {
            if let Some(diff) = data.diff_us {
                format!(" (Bounce Time: {})", util::format_us(diff))
            } else {
                " (Bounce Time: N/A)".to_string()
            }
        } else {
            "".to_string()
        };

        let near_miss_info = if !data.is_bounce && event::is_key_event(&data.event) {
            if let Some(last_us) = data.last_passed_us {
                if let Some(diff) = data.event_us.checked_sub(last_us) {
                     // Check against Duration directly, use accessor
                     if Duration::from_micros(diff) >= self.config.debounce_time() {
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


        eprintln!(
            "{} {} {} ({}, {} {}){}{}{}",
            status,
            format_relative_us(relative_us),
            type_name,
            data.event.code,
            value_name,
            data.event.value,
            key_info,
            bounce_info,
            near_miss_info
        );
    }

    /// Adapts logic from the old BounceFilter::log_simple_bounce.
    /// This is used when only `--log-bounces` is enabled.
    fn log_simple_bounce_detailed(&self, data: &EventInfo) {
        let code = data.event.code;
        let value = data.event.value;
        let type_name = get_event_type_name(data.event.type_);
        let key_name = get_key_name(code);

        let value_name = match value {
            0 => "Release",
            1 => "Press",
            2 => "Repeat",
            _ => "Unknown",
        };

        let relative_us = data
            .event_us
            .checked_sub(self.first_event_us.unwrap_or(data.event_us))
            .unwrap_or(0);

        let bounce_info = if let Some(diff) = data.diff_us {
            format!(" (Bounce Time: {})", util::format_us(diff))
        } else {
            " (Bounce Time: N/A)".to_string()
        };

        eprintln!(
            "{} {} {} ({}, {} {}) Key [{}] ({}){}",
            "[DROP]",
            format_relative_us(relative_us),
            type_name,
            code,
            value_name,
            value,
            key_name,
            code,
            bounce_info
        );
    }
}

/// Helper to format relative timestamps consistently for logging.
fn format_relative_us(relative_us: u64) -> String {
    let s = if relative_us < 1_000 {
        format!("+{} µs", relative_us)
    } else if relative_us < 1_000_000 {
        format!("+{:.1} ms", relative_us as f64 / 1000.0)
    } else {
        format!("+{:.3} s", relative_us as f64 / 1_000_000.0)
    };
    format!("{:<10}", s)
}

// format_us moved to src/util.rs
