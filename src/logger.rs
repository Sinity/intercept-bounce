// This module defines the Logger thread, which handles logging events
// and accumulating/reporting statistics based on messages received
// from the main processing thread.

use crate::event;
use crate::filter::keynames::{get_key_name, get_event_type_name};
use crate::filter::stats::StatsCollector;
use crate::config::Config;
use colored::*;
use crossbeam_channel::Receiver;
use input_linux_sys::{input_event, EV_SYN, EV_MSC}; // Import EV_SYN and EV_MSC
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use chrono::Local; // Use chrono for wallclock time

/// Represents a message sent from the main thread to the logger thread.
// #[derive(Debug)] // input_event does not implement Debug
pub enum LogMessage {
    /// Contains detailed information about a single processed event.
    Event(EventInfo),
    // Shutdown, // Could add explicit shutdown signal if needed, but channel drop works
}

/// Detailed information about a single processed event, sent to the logger.
#[derive(Clone)] // Added Clone derive
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
    /// This is needed for near-miss calculations in the logger.
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
        if self.config.verbose { eprintln!("{}", "[LOGGER] Logger instance created. Starting run loop.".dimmed()); }

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
                    if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread draining channel: Processing message after shutdown signal.".dimmed()); }
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
                // Cannot print EventInfo directly due to input_event not implementing Debug
                if self.config.verbose {
                    eprintln!("{}", format!(
                        "[DEBUG] Logger thread processing EventInfo: type={}, code={}, value={}, event_us={}, is_bounce={}, diff_us={:?}, last_passed_us={:?}",
                        data.event.type_, data.event.code, data.event.value, data.event_us, data.is_bounce, data.diff_us, data.last_passed_us
                    ).dimmed());
                }

                self.cumulative_stats.record_event_info(&data);
                self.interval_stats.record_event_info(&data);

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
        eprintln!(
            "\n{} {} {}",
            "--- Periodic Stats Dump (Wallclock:".magenta().bold(),
            Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
                .on_bright_black()
                .bright_yellow(),
            ") ---".magenta().bold()
        );

        let interval_stats_clone = self.interval_stats.clone();
        if self.config.stats_json {
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread printing periodic stats in JSON format.".dimmed()); }
            interval_stats_clone.print_stats_json(
                &*self.config, // Dereference Arc<Config> to &Config
                None,
                &mut io::stderr().lock(),
            );
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread finished printing periodic stats in JSON format.".dimmed()); }
        } else {
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread printing periodic stats in human-readable format.".dimmed()); }
            interval_stats_clone.print_stats_to_stderr(&*self.config); // Dereference Arc<Config> to &Config
            if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread finished printing periodic stats in human-readable format.".dimmed()); }
        }

        if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread resetting interval stats.".dimmed()); }
        self.interval_stats = StatsCollector::with_capacity();
        if self.config.verbose { eprintln!("{}", "[DEBUG] Logger thread interval stats reset.".dimmed()); }
    }

    /// Adapts logic from the old BounceFilter::log_event.
    fn log_event_detailed(&self, data: &EventInfo) {
        // Determine PASS/DROP status indicator.
        let status = if data.is_bounce {
            "[DROP]".on_red().white().bold()
        } else {
            "[PASS]".on_green().black().bold()
        };

        // Calculate relative timestamp based on the first event *this logger* saw.
        let relative_us = data
            .event_us
            .checked_sub(self.first_event_us.unwrap_or(data.event_us)) // Use event_us if first_event_us is None
            .unwrap_or(0); // Handle potential time going backwards

        // Get event type name with color.
        let type_name = get_event_type_name(data.event.type_)
            .on_bright_black()
            .bright_cyan()
            .bold();

        // Get key name and value name if it's a key event.
        let (key_info, value_name_colored) = if event::is_key_event(&data.event) {
            let key_name = get_key_name(data.event.code)
                .on_bright_black()
                .bright_magenta()
                .bold();
            let value_name = key_value_name(data.event.value);
            let value_name_colored = match data.event.value {
                1 => value_name.on_bright_black().bright_green().bold(), // Press
                0 => value_name.on_bright_black().bright_red().bold(), // Release
                2 => value_name.on_bright_black().bright_yellow().bold(), // Repeat
                _ => value_name.on_bright_black().dimmed(), // Unknown
            };
            (format!(" Key [{}] ({})", key_name, data.event.code), value_name_colored)
        } else {
            ("".to_string(), "".dimmed().bold()) // No key info or value name for non-key events
        };

        // Add bounce timing info if it was a dropped key event.
        let bounce_info = if data.is_bounce && event::is_key_event(&data.event) {
            if let Some(diff) = data.diff_us {
                format!(" (Bounce Time: {})", format_relative_us(diff).on_bright_black().bright_red().bold())
            } else {
                " (Bounce Time: N/A)".on_bright_black().dimmed().to_string() // Should have diff_us if is_bounce is true for key events
            }
        } else {
            "".to_string() // No bounce info for passed or non-key events
        };

        // Add near-miss info if it was a passed key event.
        let near_miss_info = if !data.is_bounce && event::is_key_event(&data.event) {
            if let Some(last_us) = data.last_passed_us {
                // Calculate diff since last passed event
                if let Some(diff) = data.event_us.checked_sub(last_us) {
                    // Check if it's within the near-miss window (e.g., debounce_us <= diff < 100ms)
                    // Note: The 100ms threshold is hardcoded in stats.rs for accumulation.
                    // Here, we just report the diff if it's >= the debounce threshold.
                    // A more precise near-miss log might check against the 100ms threshold too.
                    // For now, just show the diff if it's a passed event with a previous passed event.
                     if diff >= self.config.debounce_us { // Only show diff if it's >= debounce time (i.e., not a bounce)
                         format!(" (Diff since last passed: {})", format_relative_us(diff).on_bright_black().bright_green().bold())
                     } else {
                         // This case (diff < config.debounce_us for a passed event) should ideally not happen
                         // if the filter logic is correct, unless time went backwards.
                         "".to_string()
                     }
                } else {
                    // Time went backwards since last passed event.
                    "".to_string()
                }
            } else {
                // This is the first passed event of this type.
                "".to_string()
            }
        } else {
            "".to_string() // No near-miss info for dropped or non-key events
        };


        // Print the formatted log line to stderr.
        eprintln!(
            "{} {} {} ({}, {} {}){}{}{}",
            status,
            format_relative_us(relative_us).on_bright_black().bright_yellow().bold(), // Relative timestamp
            type_name, // Colored type name
            data.event.code,
            value_name_colored, // Colored value name
            data.event.value, // Raw value in parentheses
            key_info, // Includes key name if applicable
            bounce_info, // Includes bounce time if dropped key
            near_miss_info // Includes diff if passed key with previous passed event
        );
    }

    /// Adapts logic from the old BounceFilter::log_simple_bounce.
    /// This is used when only `--log-bounces` is enabled.
    fn log_simple_bounce_detailed(&self, data: &EventInfo) {
        // Assumes data.is_bounce is true and it's a key event.
        let code = data.event.code;
        let value = data.event.value;
        let type_name = get_event_type_name(data.event.type_)
            .on_bright_black()
            .bright_cyan()
            .bold();
        let key_name = get_key_name(code)
            .on_bright_black()
            .bright_magenta()
            .bold();

        let value_name = key_value_name(value);
        let value_name_colored = match value {
            1 => value_name.on_bright_black().bright_green().bold(), // Press
            0 => value_name.on_bright_black().bright_red().bold(), // Release
            2 => value_name.on_bright_black().bright_yellow().bold(), // Repeat
            _ => value_name.on_bright_black().dimmed(), // Unknown
        };


        // Calculate relative timestamp based on the first event *this logger* saw.
        let relative_us = data
            .event_us
            .checked_sub(self.first_event_us.unwrap_or(data.event_us))
            .unwrap_or(0);

        // Get bounce timing info.
        let bounce_info = if let Some(diff) = data.diff_us {
            format!(" (Bounce Time: {})", format_relative_us(diff).on_bright_black().bright_red().bold())
        } else {
            " (Bounce Time: N/A)".on_bright_black().dimmed().to_string() // Should have diff_us if is_bounce is true for key events
        };

        // Print the formatted log line to stderr.
        eprintln!(
            "{} {} {} ({}, {} {}) Key [{}] ({}){}",
            "[DROP]".on_red().white().bold(), // Always [DROP] for this function
            format_relative_us(relative_us).on_bright_black().bright_yellow().bold(), // Relative timestamp
            type_name, // Colored type name
            code,
            value_name_colored, // Colored value name
            value, // Raw value in parentheses
            key_name,
            code,
            bounce_info // Includes bounce time
        );
    }
}

/// Helper to format relative timestamps consistently for logging.
fn format_relative_us(relative_us: u64) -> String {
    let s = if relative_us < 1_000 {
        // Microseconds
        format!("+{} µs", relative_us)
    } else if relative_us < 1_000_000 {
        // Milliseconds with one decimal place
        format!("+{:.1} ms", relative_us as f64 / 1000.0)
    } else {
        // Seconds with three decimal places
        format!("+{:.3} s", relative_us as f64 / 1_000_000.0)
    };
    // Pad to a fixed width for alignment in logs
    format!("{:<10}", s) // Adjust padding as needed for alignment
}

/// Helper to get descriptive name for key event value (0, 1, 2).
fn key_value_name(value: i32) -> &'static str {
    match value {
        0 => "Release",
        1 => "Press",
        2 => "Repeat",
        _ => "Unknown",
    }
}
