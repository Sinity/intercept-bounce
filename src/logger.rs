//! Logger thread implementation.
//!
//! Handles receiving event processing details from the main thread,
//! accumulating statistics, performing event logging to stderr,
//! and managing periodic stats dumps.

use crate::event; // Need event::is_key_event
use crate::filter::keynames::{get_event_type_name, get_key_name};
use crate::filter::stats::{self, StatsCollector}; // Use stats module
use colored::*;
use crossbeam_channel::Receiver;
use input_linux_sys::{input_event, EV_SYN}; // Need EV_SYN
use std::io::{self, Write}; // For stderr access and Write trait
use std::sync::atomic::{AtomicBool, Ordering}; // Added for logger_running flag
use std::sync::Arc; // Added for Arc<AtomicBool>
use std::time::{Duration, Instant};

/// Messages sent from the Main processing thread to the Logger thread.
// Removed Debug derive as input_event doesn't implement it
pub enum LogMessage {
    /// Contains detailed information about a single processed event.
    Event(EventInfo),
    // Shutdown, // Could add explicit shutdown signal if needed
}

/// Detailed information about a processed event, sent to the logger thread.
// Removed Debug derive as input_event doesn't implement it
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
    /// Timestamp (µs) of the *previous* event of the same key code and value
    /// that passed the filter, or `None` if this was the first. Used for near-miss calculation.
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
    debounce_time_us: u64, // Needed for printing stats context

    // Logger owns and manages all statistics collectors.
    cumulative_stats: StatsCollector,
    interval_stats: StatsCollector,

    // State for periodic dumping.
    last_periodic_dump: Instant,
    // Timestamp of the first event seen by the logger (for relative logging).
    // Note: This might differ slightly from BounceFilter's overall_first_event_us
    // if the first message is dropped, but okay for relative log timestamps.
    logger_first_event_us: Option<u64>,
    // Flag to track if the last logged line was for a SYN event (for grouping).
    last_log_was_syn: bool,
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
        debounce_us: u64,
    ) -> Self {
        Logger {
            receiver,
            logger_running,
            log_all_events: log_all,
            log_bounces,
            // Use Duration::MAX to effectively disable periodic logging if interval is 0.
            log_interval: if interval_s > 0 {
                Duration::from_secs(interval_s)
            } else {
                Duration::MAX
            },
            stats_json: json,
            debounce_time_us: debounce_us,
            cumulative_stats: StatsCollector::with_capacity(),
            interval_stats: StatsCollector::with_capacity(),
            last_periodic_dump: Instant::now(), // Start timing immediately
            logger_first_event_us: None,
            last_log_was_syn: true, // Assume initial state allows header
        }
    }

    /// Runs the logger thread's main loop.
    ///
    /// Listens for messages, updates stats, performs logging, handles periodic dumps.
    /// Exits when the input channel is disconnected or the `logger_running` flag is set to false.
    /// Returns the final cumulative statistics upon exit.
    pub fn run(&mut self) -> StatsCollector { // Removed info print
        // How often to check the timer/channel when idle.
        let check_interval = Duration::from_millis(100);

        loop {
            // --- Check explicit shutdown signal first ---
            // This allows the logger to exit quickly even if the channel still has messages
            // or if the main thread exited without dropping the sender cleanly (e.g., panic).
            if !self.logger_running.load(Ordering::SeqCst) {
                eprintln!("{}", "[DEBUG] Logger thread received shutdown signal via AtomicBool, exiting.".dimmed());
                break;
            }

            // --- Check for Periodic Stats ---
            // Only dump if interval is enabled and elapsed time is sufficient.
            if self.log_interval != Duration::MAX
                && self.last_periodic_dump.elapsed() >= self.log_interval
            {
                self.dump_periodic_stats();
                // Reset interval stats and timer *after* dumping.
                self.interval_stats = StatsCollector::with_capacity(); // Reset by creating new
                self.last_periodic_dump = Instant::now();
            }

            // --- Receive Log Messages ---
            // Use recv_timeout to periodically check the timer and the running flag
            // even if no messages arrive.
            match self.receiver.recv_timeout(check_interval) {
                Ok(LogMessage::Event(data)) => {
                    // Track the first event timestamp seen by the logger.
                    if self.logger_first_event_us.is_none() {
                        self.logger_first_event_us = Some(data.event_us);
                    }

                    // --- Accumulate Stats ---
                    // Update both cumulative and interval statistics.
                    self.cumulative_stats.record_event_info(&data);
                    self.interval_stats.record_event_info(&data);

                    // --- Log Event (if needed) ---
                    let is_syn = data.event.type_ == EV_SYN as u16;
                    if self.log_all_events {
                        // Print header if needed before logging the event packet.
                        if self.last_log_was_syn && !is_syn {
                             eprintln!(
                                 "{}",
                                 "--- Event Packet ---".on_bright_black().bold().underline().truecolor(255, 255, 0)
                             );
                        }
                        self.log_event_detailed(&data);
                    } else if self.log_bounces && data.is_bounce && event::is_key_event(&data.event) {
                        // Only log key event bounces if log_all is off.
                        self.log_simple_bounce_detailed(&data);
                    }
                    self.last_log_was_syn = is_syn; // Update SYN flag
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // No message received within the timeout. Loop again to check timer/flag.
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    // Main thread dropped the sender. This is a valid shutdown signal,
                    // but the AtomicBool check is the primary mechanism now.
                    eprintln!("{}", "[DEBUG] Logger channel disconnected.".dimmed());
                    break; // Exit the loop gracefully.
                }
            }
        }
        // Return the final cumulative stats when the thread exits.
        // Use take to move ownership out of self, replacing with default.
        std::mem::take(&mut self.cumulative_stats)
    }

    /// Dumps the current interval statistics to stderr.
    fn dump_periodic_stats(&self) {
        // Print header with wallclock time.
        eprintln!(
            "\n{} {} {}",
            "--- Periodic Stats Dump (Wallclock:".magenta().bold(),
            chrono::Local::now() // Use chrono for wallclock time
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string()
                .on_bright_black()
                .bright_yellow()
                .bold(),
            ") ---".magenta().bold()
        );

        // Use the current interval_stats for printing.
        if self.stats_json {
            // Calculate interval runtime based on wall clock timer.
            let interval_runtime_us = self.last_periodic_dump.elapsed().as_micros() as u64;
            eprintln!(
                "{}",
                "Interval stats (since last dump):"
                    .on_bright_black()
                    .bold()
                    .bright_white()
            );
            // Print interval stats as JSON.
            self.interval_stats.print_stats_json(
                self.debounce_time_us,
                self.log_all_events,
                self.log_bounces,
                self.log_interval.as_micros() as u64, // Convert Duration back to us
                Some(interval_runtime_us),
                &mut io::stderr().lock(), // Lock stderr for writing
            );
            // Optionally print cumulative stats snapshot too in JSON periodic dump
            // eprintln!("{}", "Cumulative stats snapshot:".on_bright_black().bold().bright_white());
            // self.cumulative_stats.print_stats_json(...)
        } else {
            eprintln!(
                "{}",
                "Interval stats (since last dump):"
                    .on_bright_black()
                    .bold()
                    .bright_white()
            );
            // Print interval stats in human-readable format.
            self.interval_stats.print_stats_to_stderr(
                self.debounce_time_us,
                self.log_all_events,
                self.log_bounces,
                self.log_interval.as_micros() as u64,
            );
            // Optionally print cumulative stats snapshot too
            // eprintln!("{}", "Cumulative stats snapshot:".on_bright_black().bold().bright_white());
            // self.cumulative_stats.print_stats_to_stderr(...)
        }
        eprintln!(
            "{}",
            "-------------------------------------------\n"
                .magenta()
                .bold()
        );
    }

    /// Logs detailed information about any event (if log_all_events is true).
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
            .saturating_sub(self.logger_first_event_us.unwrap_or(data.event_us));
        // Format relative time (e.g., "+123.4 ms").
        let relative_time_str = format_relative_us(relative_us)
            .on_bright_black()
            .bright_yellow()
            .bold();

        // Get human-readable event type name.
        let type_name = get_event_type_name(data.event.type_)
            .on_bright_black()
            .bright_cyan()
            .bold();

        let mut event_details = String::new();
        let mut timing_info = String::new();

        // Format details differently for key events vs other events.
        if event::is_key_event(&data.event) {
            let key_code = data.event.code;
            let key_value = data.event.value;
            let key_name = get_key_name(key_code)
                .on_bright_black()
                .bright_magenta()
                .bold();
            let code_str = format!("{}", key_code).bright_blue().bold();
            let value_str = format!("{}", key_value).bright_yellow().bold();
            event_details.push_str(&format!("[{}] ({}, {})", key_name, code_str, value_str));

            // Add timing information based on bounce status and previous event time.
            if data.is_bounce {
                if let Some(diff) = data.diff_us {
                    timing_info.push_str(&format!(
                        " {} {}",
                        "Bounce Diff:".on_bright_black().bright_red().bold(),
                        stats::format_us(diff).on_bright_black().bright_red().bold()
                    ));
                }
            } else if let Some(prev) = data.last_passed_us {
                let time_since_last_passed = data.event_us.saturating_sub(prev);
                timing_info.push_str(&format!(
                    " {} {}",
                    "Time since last passed:".on_bright_black().bright_green().bold(),
                    stats::format_us(time_since_last_passed)
                        .on_bright_black()
                        .bright_green()
                        .bold()
                ));
            } else {
                // Indicate if this was the first passed event of its type.
                timing_info.push_str(&format!(
                    " {}", // Add space separator
                    "First passed event of this type"
                        .on_bright_black()
                        .dimmed() // Dimmed for less emphasis
                        .to_string()
                ));
            }
        } else {
            // Format for non-key events (e.g., EV_SYN).
            let code_str = format!("{}", data.event.code).bright_blue().bold();
            let value_str = format!("{}", data.event.value).bright_yellow().bold();
            event_details.push_str(&format!("Code: {}, Value: {}", code_str, value_str));
        }

        // Pad details for alignment.
        let padded_details = format!("{:<30}", event_details).on_bright_black().white();
        let indentation = "  "; // Indent log lines slightly.

        // Print the fully formatted log line to stderr.
        // Lock stderr for the duration of the print.
        let mut stderr = io::stderr().lock();
        let _ = writeln!(
            stderr,
            "{}{}{} {} ({}) {}{}",
            indentation,
            status,
            relative_time_str,
            type_name,
            data.event.type_, // Include raw type code
            padded_details,
            timing_info
        );
    }

    /// Logs minimal information about a dropped key event (if log_bounces is true).
    /// Adapts logic from the old BounceFilter::log_simple_bounce.
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
        let code_str = format!("{}", code).bright_blue().bold();
        let value_str = format!("{}", value).bright_yellow().bold();

        // Lock stderr for the duration of the print.
        let mut stderr = io::stderr().lock();

        // Print basic drop info.
        let _ = write!(
            stderr,
            "{} {} {} {}, Type: {} ({}), Code: {} [{}], Value: {}",
            "[DROP]".on_red().white().bold(), // Status
            data.event_us.to_string().on_bright_black().bright_yellow().bold(), // Timestamp
            "µs".on_bright_black().bright_yellow().bold(), // Units
            " ".on_bright_black(), // Separator
            type_name,
            data.event.type_,
            code_str,
            key_name,
            value_str
        );
        // Add bounce difference if available.
        if let Some(diff) = data.diff_us {
            let _ = write!(
                stderr,
                ", {} {}",
                "Bounce Diff:".on_bright_black().bright_red().bold(),
                stats::format_us(diff).on_bright_black().bright_red().bold()
            );
        }
        let _ = writeln!(stderr); // Newline
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
    // Pad to a fixed width for alignment in logs.
    format!("{:>12}", s)
}
