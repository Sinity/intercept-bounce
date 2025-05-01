// This module defines the Logger thread, which handles logging events
// and accumulating/reporting statistics based on messages received
// from the main processing thread.

use crate::event;
use crate::filter::keynames::{get_key_name, get_event_type_name};
use crate::filter::stats::StatsCollector;
use crate::config::Config;
use colored::*;
use crossbeam_channel::Receiver;
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
        if self.config.verbose { eprintln!("{}", "[LOGGER] Logger thread started.".dimmed()); }
        let log_interval = if self.config.log_interval_us > 0 {
            Duration::from_micros(self.config.log_interval_us)
        } else {
            Duration::MAX
        };
        let check_interval = Duration::from_millis(100);

        loop {
            if !self.logger_running.load(Ordering::SeqCst) {
                if self.config.verbose { eprintln!("{}", "[LOGGER] Received shutdown signal via AtomicBool, attempting to drain channel.".dimmed()); }
                while let Ok(msg) = self.receiver.try_recv() {
                    if self.config.verbose { eprintln!("{}", "[LOGGER] Draining channel: Processing message after shutdown signal.".dimmed()); }
                    self.process_message(msg);
                }
                if self.config.verbose { eprintln!("{}", "[LOGGER] Finished draining channel. Exiting run loop.".dimmed()); }
                break;
            }

            if log_interval != Duration::MAX && self.last_dump_time.elapsed() >= log_interval {
                if self.config.verbose { eprintln!("{}", "[LOGGER] Triggering periodic stats dump.".dimmed()); }
                self.dump_periodic_stats();
                self.last_dump_time = Instant::now();
                if self.config.verbose { eprintln!("{}", "[LOGGER] Periodic stats dump complete. Timer reset.".dimmed()); }
            }

            match self.receiver.recv_timeout(check_interval) {
                Ok(msg) => {
                    if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread received message from channel.".dimmed()); }
                    self.process_message(msg);
                    if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread finished processing message.".dimmed()); }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread receive timed out. Re-checking flags.".dimmed()); }
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    eprintln!("{}", "[LOGGER] Detected channel disconnected. Attempting to drain channel.".dimmed());
                    while let Ok(msg) = self.receiver.try_recv() {
                        if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread draining channel: Processing message after disconnect.".dimmed()); }
                        self.process_message(msg);
                    }
                    eprintln!("{}", "[LOGGER] Finished draining channel. Exiting run loop.".dimmed());
                    break;
                }
            }
        }

        if self.config.verbose { eprintln!("{}", "[LOGGER] Run loop exited. Preparing final stats.".dimmed()); }
        if self.config.verbose { eprintln!("{}", "[LOGGER] Taking cumulative_stats for return.".dimmed()); }
        std::mem::take(&mut self.cumulative_stats)
    }

    /// Processes a single message received from the main thread.
    /// Updates statistics and performs logging if enabled.
    pub fn process_message(&mut self, msg: LogMessage) { // Made public for benches/tests
        match msg {
            LogMessage::Event(data) => {
                // Cannot print EventInfo directly due to input_event not implementing Debug.
                if self.config.verbose {
                    eprintln!("{}", format!(
                        "[DEBUG] Logger thread processing EventInfo: type={}, code={}, value={}, event_us={}, is_bounce={}, diff_us={:?}, last_passed_us={:?}",
                        data.event.type_, data.event.code, data.event.value, data.event_us, data.is_bounce, data.diff_us, data.last_passed_us
                    ).dimmed());
                }

                self.cumulative_stats.record_event_info_with_config(&data, &self.config);
                self.interval_stats.record_event_info_with_config(&data, &self.config);

                if self.first_event_us.is_none() {
                    self.first_event_us = Some(data.event_us);
                    if self.config.verbose { eprintln!("{}", format!("[DEBUG] Logger thread recorded first event timestamp: {}", data.event_us).dimmed()); }
                }

                if self.config.log_all_events {
                    if data.event.type_ == EV_SYN as u16 || data.event.type_ == EV_MSC as u16 {
                        return;
                    }
                    if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread logging all events.".dimmed()); }
                    self.log_event_detailed(&data);
                } else if self.config.log_bounces && data.is_bounce && event::is_key_event(&data.event) {
                    if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread logging bounce event.".dimmed()); }
                    self.log_simple_bounce_detailed(&data);
                }
            }
        }
    }

    /// Dumps the current interval statistics to stderr.
    fn dump_periodic_stats(&mut self) {
        eprintln!("\n--- Periodic Stats Dump (Wallclock: {}) ---",
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string()
        );

        let interval_stats_clone = self.interval_stats.clone();
        if self.config.stats_json {
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread printing periodic stats in JSON format.".dimmed()); }
            interval_stats_clone.print_stats_json(
                &*self.config,
                None,
                &mut io::stderr().lock(),
            );
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread finished printing periodic stats in JSON format.".dimmed()); }
        } else {
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread printing periodic stats in human-readable format.".dimmed()); }
            interval_stats_clone.print_stats_to_stderr(&*self.config);
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread finished printing periodic stats in human-readable format.".dimmed()); }
        }

        if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread resetting interval stats.".dimmed()); }
        self.interval_stats = StatsCollector::with_capacity();
        if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread interval stats reset.".dimmed()); }
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
                format!(" (Bounce Time: {})", format_us(diff))
            } else {
                " (Bounce Time: N/A)".to_string()
            }
        } else {
            "".to_string()
        };

        let near_miss_info = if !data.is_bounce && event::is_key_event(&data.event) {
            if let Some(last_us) = data.last_passed_us {
                if let Some(diff) = data.event_us.checked_sub(last_us) {
                     if diff >= self.config.debounce_us {
                         format!(" (Diff since last passed: {})", format_us(diff))
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
            format!(" (Bounce Time: {})", format_us(diff))
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

/// Helper to format a duration in microseconds into a human-readable string (µs or ms).
// This is duplicated from stats.rs, but kept here for logger's internal formatting.
#[inline]
fn format_us(us: u64) -> String {
    if us < 1000 {
        format!("{} µs", us)
    } else if us < 1_000_000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{:.3} s", us as f64 / 1_000_000.0)
    }
}
