// This module defines the Logger thread, which handles logging events
// and accumulating/reporting statistics based on messages received
// from the main processing thread.

use crate::event;
use crate::filter::keynames::{get_key_name, get_event_type_name};
use crate::filter::stats::StatsCollector;
use crate::config::Config;
use crate::util; // Import util
// Conditionally import and define types
#[cfg(not(feature = "use_lockfree_queue"))]
use crossbeam_channel::{Receiver, RecvTimeoutError};
#[cfg(feature = "use_lockfree_queue")]
use crossbeam_queue::ArrayQueue;

use input_linux_sys::{input_event, EV_SYN, EV_MSC};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread; // Add thread import for yield_now/sleep
use std::time::{Duration, Instant};
use chrono::Local;
use tracing::info; // Only info is used directly in this file's functions

// Conditionally define the receiver type alias
#[cfg(not(feature = "use_lockfree_queue"))]
type LogReceiver = Receiver<LogMessage>;
#[cfg(feature = "use_lockfree_queue")]
type LogReceiver = Arc<ArrayQueue<LogMessage>>; // Receiver is the Arc reference


/// Represents a message sent from the main thread to the logger thread.
#[derive(Clone)] // Add Clone for ArrayQueue push
// #[derive(Debug)] // input_event does not implement Debug
pub enum LogMessage {
    /// Contains detailed information about a single processed event.
    Event(EventInfo),
}

/// Detailed information about a single processed event, sent to the logger.
#[derive(Clone)] // Add Clone for ArrayQueue push
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
    receiver: LogReceiver, // Use the alias
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
        receiver: LogReceiver, // Use the alias
        logger_running: Arc<AtomicBool>,
        config: Arc<Config>,
    ) -> Self {
        Logger {
            receiver, // Use the alias
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
        tracing::debug!("Logger thread started");
        let log_interval = self.config.log_interval(); // Get Duration directly
        let check_interval = Duration::from_millis(100); // Used for periodic checks

        loop {
            let mut work_done = false; // Track if we processed a message in this iteration

            // --- Conditionally receive messages ---
            #[cfg(not(feature = "use_lockfree_queue"))]
            {
                // Use recv_timeout for crossbeam-channel
                match self.receiver.recv_timeout(check_interval) {
                    Ok(msg) => {
                        tracing::trace!("Logger thread received message from channel");
                        self.process_message(msg);
                        work_done = true;
                        tracing::trace!("Logger thread finished processing message");
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        // No message received, continue to check flags/timer
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        tracing::warn!("Detected channel disconnected. Attempting to drain channel");
                        // Drain logic remains similar for disconnected channel
                        while let Ok(msg) = self.receiver.try_recv() {
                            self.process_message(msg);
                        }
                        tracing::warn!("Finished draining channel. Exiting run loop");
                        break; // Exit loop on disconnect
                    }
                }
            } // End cfg block for crossbeam-channel

            #[cfg(feature = "use_lockfree_queue")]
            {
                // Use non-blocking pop for ArrayQueue
                while let Some(msg) = self.receiver.pop() {
                     tracing::trace!("Logger thread received message from queue");
                     self.process_message(msg);
                     work_done = true;
                     tracing::trace!("Logger thread finished processing message");
                }
                // Note: ArrayQueue doesn't signal disconnect directly.
                // We rely on the logger_running flag and main thread dropping its Arc.
            } // End cfg block for ArrayQueue
            // --- End conditional receive ---


            // Check running flag *after* attempting to process messages
            if !self.logger_running.load(Ordering::SeqCst) {
                tracing::debug!("Received shutdown signal via AtomicBool, attempting final drain");
                // Drain logic for ArrayQueue needs pop loop
                #[cfg(feature = "use_lockfree_queue")]
                while let Some(msg) = self.receiver.pop() {
                     tracing::trace!("Logger thread draining queue: Processing message after shutdown");
                     self.process_message(msg);
                }
                // Drain logic for crossbeam-channel (already handled disconnect case above)
                #[cfg(not(feature = "use_lockfree_queue"))]
                while let Ok(msg) = self.receiver.try_recv() {
                     tracing::trace!("Logger thread draining channel: Processing message after shutdown");
                     self.process_message(msg);
                }
                tracing::debug!("Finished final drain. Exiting run loop");
                break; // Exit loop on shutdown signal
            }

            // Check periodic stats dump timer
            if log_interval > Duration::ZERO && self.last_dump_time.elapsed() >= log_interval {
                tracing::debug!("Triggering periodic stats dump");
                self.dump_periodic_stats();
                self.last_dump_time = Instant::now();
                work_done = true; // Dumping stats counts as work
                tracing::debug!("Periodic stats dump complete. Timer reset");
            }

            // If no message was processed and no timer fired, yield/sleep briefly
            // to avoid busy-waiting, especially with ArrayQueue's non-blocking pop.
            if !work_done {
                tracing::trace!("Logger thread yielding/sleeping briefly");
                thread::sleep(check_interval / 10); // Sleep for a fraction of the check interval
            }

        } // End loop

        tracing::debug!("Run loop exited. Preparing final stats");
        tracing::debug!("Taking cumulative_stats for return");
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
                    tracing::trace!("Logger logging all events");
                    self.log_event_detailed(&data);
                } else if self.config.log_bounces && data.is_bounce && event::is_key_event(&data.event) {
                    tracing::trace!("Logger logging bounce event");
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
            tracing::debug!("Logger thread printing periodic stats in JSON format");
            interval_stats_clone.print_stats_json(
                &*self.config,
                None, // Runtime is only for cumulative
                "Periodic", // Report type
                &mut io::stderr().lock(), // Write directly to stderr
            );
            tracing::debug!("Logger thread finished printing periodic stats in JSON format");
        } else {
            tracing::debug!("Logger thread printing periodic stats in human-readable format");
            interval_stats_clone.print_stats_to_stderr(&*self.config, "Periodic"); // Pass config and type
            tracing::debug!("Logger thread finished printing periodic stats in human-readable format");
        }

        tracing::debug!("Logger thread resetting interval stats");
        self.interval_stats = StatsCollector::with_capacity();
        tracing::debug!("Logger thread interval stats reset");
    }

    /// Adapts logic from the old BounceFilter::log_event.
    /// Logs details of a single event (passed or dropped) using tracing.
    fn log_event_detailed(&self, data: &EventInfo) {
        let status = if data.is_bounce {
            "DROP"
        } else {
            "PASS"
        };

        let relative_us = data
            .event_us
            .checked_sub(self.first_event_us.unwrap_or(data.event_us))
            .unwrap_or(0);

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
                     if Duration::from_micros(diff) >= self.config.debounce_time() && Duration::from_micros(diff) <= self.config.near_miss_threshold() {
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

        let relative_us = data
            .event_us
            .checked_sub(self.first_event_us.unwrap_or(data.event_us))
            .unwrap_or(0);

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
        format!("+{} µs", relative_us)
    } else if relative_us < 1_000_000 {
        format!("+{:.1} ms", relative_us as f64 / 1000.0)
    } else {
        format!("+{:.3} s", relative_us as f64 / 1_000_000.0)
    };
    format!("{:<10}", s)
}

// format_us moved to src/util.rs
