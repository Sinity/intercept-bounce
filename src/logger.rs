// This module defines the Logger thread, which handles logging events
// and accumulating/reporting statistics based on messages received
// from the main processing thread.

use crate::event::{self, get_event_type_name}; // Use event module functions
use crate::filter::keynames::get_key_name;
use crate::filter::stats::{StatsCollector, Meta}; // Import StatsCollector and Meta
use colored::*;
use crossbeam_channel::Receiver;
use input_linux_sys::input_event;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use chrono::Local; // Use chrono for wallclock time

/// Represents a message sent from the main thread to the logger thread.
#[derive(Debug)] // Add Debug derive
pub enum LogMessage {
    /// Contains detailed information about a single processed event.
    Event(EventInfo),
    // Shutdown, // Could add explicit shutdown signal if needed, but channel drop works
}

/// Detailed information about a single processed event, sent to the logger.
#[derive(Debug)] // Add Debug derive
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
    // Channel receiver for messages from the main thread.
    receiver: Receiver<LogMessage>,
    // Shared flag to signal logger thread termination.
    logger_running: Arc<AtomicBool>,
    // Configuration flags passed from main.
    log_all_events: bool,
    log_bounces: bool,
    log_interval: Duration,
    stats_json: bool,
    debounce_us: u64, // Store debounce time in µs for near-miss check

    // Statistics collectors.
    // `cumulative_stats` holds totals for the entire run.
    cumulative_stats: StatsCollector,
    // `interval_stats` holds totals since the last periodic dump.
    interval_stats: StatsCollector,

    // State for periodic logging.
    last_dump_time: Instant,
    // Timestamp of the first event seen by *this logger thread*. Used for relative timestamps.
    first_event_us: Option<u64>,
}

impl Logger {
    /// Creates a new Logger instance.
    pub fn new(
        receiver: Receiver<LogMessage>,
        logger_running: Arc<AtomicBool>, // Receive the shared flag
        log_all: bool,
        log_bounces: bool,
        interval_s: u64,
        json: bool,
        debounce_us: u64, // Receive debounce time in µs
    ) -> Self {
        let log_interval = if interval_s > 0 {
            Duration::from_secs(interval_s)
        } else {
            Duration::MAX // Effectively disabled
        };

        Logger {
            receiver,
            logger_running,
            log_all_events: log_all,
            log_bounces: log_bounces && !log_all, // log_bounces is ignored if log_all is true
            log_interval,
            stats_json: json,
            debounce_us, // Store debounce time

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
    pub fn run(&mut self) -> StatsCollector { // Removed info print
        // How often to check the timer/channel when idle.
        let check_interval = Duration::from_millis(100);

        loop {
            // --- Check explicit shutdown signal first ---
            // This allows the logger to exit quickly even if the channel still has messages
            // or if the main thread exited without dropping the sender cleanly (e.g., panic).
            if !self.logger_running.load(Ordering::SeqCst) {
                eprintln!("{}", "[DEBUG] Logger thread received shutdown signal via AtomicBool, attempting to drain channel.".dimmed());
                // Signal received, try to drain the channel before exiting.
                // This ensures we process any messages sent just before the signal.
                while let Ok(msg) = self.receiver.try_recv() {
                    eprintln!("{}", "[DEBUG] Logger thread draining channel: Processing message after shutdown signal.".dimmed());
                    self.process_message(msg);
                }
                eprintln!("{}", "[DEBUG] Logger thread finished draining channel. Exiting run loop.".dimmed());
                break; // Exit the loop
            }

            // --- Check for periodic dump ---
            if self.log_interval != Duration::MAX && self.last_dump_time.elapsed() >= self.log_interval {
                eprintln!("{}", "[DEBUG] Logger thread triggering periodic stats dump.".dimmed());
                self.dump_periodic_stats();
                self.last_dump_time = Instant::now(); // Reset timer
                eprintln!("{}", "[DEBUG] Logger thread periodic stats dump complete. Timer reset.".dimmed());
            }

            // --- Receive and process messages ---
            // Use `try_recv` with a timeout to allow checking the running flag and timer.
            eprintln!("{}", format!("[DEBUG] Logger thread attempting to receive message with timeout: {:?}", check_interval).dimmed());
            match self.receiver.recv_timeout(check_interval) {
                Ok(msg) => {
                    eprintln!("{}", "[DEBUG] Logger thread received message from channel.".dimmed());
                    self.process_message(msg);
                    eprintln!("{}", "[DEBUG] Logger thread finished processing message.".dimmed());
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // No message received within the timeout. Loop continues,
                    // checking the running flag and timer again.
                    eprintln!("{}", "[DEBUG] Logger thread receive timed out. Re-checking flags.".dimmed());
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    // Sender was dropped, meaning the main thread has exited.
                    eprintln!("{}", "[DEBUG] Logger thread detected channel disconnected. Attempting to drain channel.".dimmed());
                    // Drain any remaining messages before exiting.
                    while let Ok(msg) = self.receiver.try_recv() {
                         eprintln!("{}", "[DEBUG] Logger thread draining channel: Processing message after disconnect.".dimmed());
                         self.process_message(msg);
                    }
                    eprintln!("{}", "[DEBUG] Logger thread finished draining channel. Exiting run loop.".dimmed());
                    break; // Exit the loop
                }
            }
        }

        eprintln!("{}", "[DEBUG] Logger thread run loop exited. Preparing final stats.".dimmed());
        // The loop has exited. Print final cumulative stats.
        // The main thread will wait for this thread to join and then print the stats.
        // We return the cumulative stats collector.
        // Use std::mem::take to move the cumulative_stats out, leaving a default in self.
        eprintln!("{}", "[DEBUG] Logger thread taking cumulative_stats for return.".dimmed());
        std::mem::take(&mut self.cumulative_stats)
    }

    /// Processes a single message received from the main thread.
    /// Updates statistics and performs logging if enabled.
    fn process_message(&mut self, msg: LogMessage) {
        eprintln!("{}", "[DEBUG] Logger thread processing message.".dimmed());
        match msg {
            LogMessage::Event(data) => {
                eprintln!("{}", format!("[DEBUG] Logger thread processing EventInfo: {:?}", data).dimmed());
                // Record stats for both cumulative and interval collectors.
                self.cumulative_stats.record_event_info(&data);
                self.interval_stats.record_event_info(&data);

                // Set the first event timestamp if not already set.
                if self.first_event_us.is_none() {
                    self.first_event_us = Some(data.event_us);
                    eprintln!("{}", format!("[DEBUG] Logger thread recorded first event timestamp: {}", data.event_us).dimmed());
                }

                // Perform logging based on flags.
                if self.log_all_events {
                    eprintln!("{}", "[DEBUG] Logger thread logging all events.".dimmed());
                    self.log_event_detailed(&data);
                } else if self.log_bounces && data.is_bounce && event::is_key_event(&data.event) {
                    // Only log bounces if log_all_events is false, log_bounces is true,
                    // it's a bounce, and it's a key event.
                    eprintln!("{}", "[DEBUG] Logger thread logging bounce event.".dimmed());
                    self.log_simple_bounce_detailed(&data);
                }
            }
        }
        eprintln!("{}", "[DEBUG] Logger thread finished processing message.".dimmed());
    }

    /// Dumps the current interval statistics to stderr.
    fn dump_periodic_stats(&self) {
        eprintln!("{}", "[DEBUG] Logger thread dumping periodic stats.".dimmed());
        // Print header with wallclock time.
        eprintln!(
            "\n{} {} {}",
            "--- Periodic Stats Dump (Wallclock:".magenta().bold(),
            chrono::Local::now() // Use chrono for wallclock time
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
                .on_bright_black()
                .bright_yellow(),
            ") ---".magenta().bold()
        );

        // Create a temporary Meta struct for the dump.
        let meta = Meta {
            debounce_time_us: self.debounce_us,
            log_all_events: self.log_all_events,
            log_bounces: self.log_bounces,
            log_interval_us: self.log_interval.as_micros() as u64, // Convert Duration to u64 µs
        };

        // Note: Runtime is not included in periodic dumps as it's a cumulative value.
        // Pass None for runtime_us.
        if self.stats_json {
            eprintln!("{}", "[DEBUG] Logger thread printing periodic stats in JSON format.".dimmed());
            // Use a temporary StatsCollector for the interval stats.
            // We need to clone it because print_stats_json takes &self.
            // A more efficient approach might be to pass the interval_stats directly
            // if print_stats_json took a mutable reference or was a method on Logger.
            // For now, cloning is acceptable for periodic dumps which are infrequent.
            let interval_stats_clone = self.interval_stats.clone();
             interval_stats_clone.print_stats_json(
                self.debounce_us,
                self.log_all_events,
                self.log_bounces,
                self.log_interval.as_micros() as u64,
                None, // No runtime for periodic dump
                &mut io::stderr().lock(), // Lock stderr for writing
            );
            eprintln!("{}", "[DEBUG] Logger thread finished printing periodic stats in JSON format.".dimmed());
        } else {
            eprintln!("{}", "[DEBUG] Logger thread printing periodic stats in human-readable format.".dimmed());
            // Use a temporary StatsCollector for the interval stats.
            let interval_stats_clone = self.interval_stats.clone();
            interval_stats_clone.print_stats_to_stderr(
                self.debounce_us,
                self.log_all_events,
                self.log_bounces,
                self.log_interval.as_micros() as u64,
            );
            eprintln!("{}", "[DEBUG] Logger thread finished printing periodic stats in human-readable format.".dimmed());
        }

        // Reset interval stats after dumping.
        eprintln!("{}", "[DEBUG] Logger thread resetting interval stats.".dimmed());
        self.interval_stats = StatsCollector::with_capacity();
        eprintln!("{}", "[DEBUG] Logger thread interval stats reset.".dimmed());
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

        // Get event type name.
        let type_name = get_event_type_name(data.event.type_)
            .on_bright_black()
            .bright_cyan()
            .bold();

        // Get key name if it's a key event.
        let key_info = if event::is_key_event(&data.event) {
            let key_name = get_key_name(data.event.code)
                .on_bright_black()
                .bright_magenta()
                .bold();
            format!(" Key [{}] ({})", key_name, data.event.code)
        } else {
            "".to_string() // No key info for non-key events
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
                    // Here, we just report the diff if it's > debounce_us.
                    // A more precise near-miss log might check against the 100ms threshold too.
                    // For now, just show the diff if it's a passed event with a previous passed event.
                     if diff >= self.debounce_us { // Only show diff if it's >= debounce time (i.e., not a bounce)
                         format!(" (Diff since last passed: {})", format_relative_us(diff).on_bright_black().bright_green().bold())
                     } else {
                         // This case (diff < debounce_us for a passed event) should ideally not happen
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
            "{} {} {} ({}, {}){}{}{}",
            status,
            format_relative_us(relative_us).on_bright_black().bright_yellow().bold(), // Relative timestamp
            type_name,
            data.event.code,
            data.event.value,
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
            "{} {} {} ({}, {}) Key [{}] ({}){}",
            "[DROP]".on_red().white().bold(), // Always [DROP] for this function
            format_relative_us(relative_us).on_bright_black().bright_yellow().bold(), // Relative timestamp
            type_name,
            code,
            value,
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
