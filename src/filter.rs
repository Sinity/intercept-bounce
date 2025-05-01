mod keynames;
pub mod stats;

use crate::event::{event_microseconds, is_key_event};
use input_linux_sys::{input_event, EV_SYN};
// Removed unused HashMap import
use std::io::{self, Write};
use colored::*;
use chrono;

use keynames::{get_key_name, get_event_type_name};
use stats::StatsCollector;

// Unit tests moved to tests/filter_tests.rs

/// Holds the state for bounce filtering.
pub struct BounceFilter {
    pub debounce_time_us: u64,
    pub log_interval_us: u64,
    pub log_all_events: bool,
    pub log_bounces: bool,
    pub stats_json: bool, // Store the flag here
    last_event_us: Box<[[u64; 3]; 1024]>, // [keycode][value] = last passed event timestamp
    last_any_event_us: Box<[u64; 1024]>, // [keycode] = last event timestamp (any value)
    pub overall_first_event_us: Option<u64>, // Timestamp of the very first event processed
    pub overall_last_event_us: Option<u64>,  // Timestamp of the very last event processed
    last_event_was_syn: bool, // Flag to help group log output
    pub stats: StatsCollector, // Cumulative statistics
    interval_stats: StatsCollector, // reset after each interval dump
    last_stats_dump_time_us: Option<u64>,
}

impl BounceFilter {
    pub fn new(debounce_time_ms: u64, log_interval_s: u64, log_all_events: bool, log_bounces: bool) -> Self {
        BounceFilter {
            debounce_time_us: debounce_time_ms * 1_000,
            log_interval_us: log_interval_s * 1_000_000,
            log_all_events,
            log_bounces,
            // Initialize with MAX to distinguish from timestamp 0
            last_event_us: Box::new([[u64::MAX; 3]; 1024]),
            last_any_event_us: Box::new([u64::MAX; 1024]),
            overall_first_event_us: None,
            overall_last_event_us: None,
            last_event_was_syn: true,
            stats: StatsCollector::with_capacity(),
            interval_stats: StatsCollector::with_capacity(),
            last_stats_dump_time_us: None,
        }
    }

    // Use the shared format_us from stats.rs for consistency

    fn format_timestamp_relative(relative_us: u64) -> String {
        let s = if relative_us < 1_000 {
            format!("+{} µs", relative_us)
        } else if relative_us < 1_000_000 {
            format!("+{:.1} ms", relative_us as f64 / 1000.0)
        } else {
            format!("+{:.3} s", relative_us as f64 / 1_000_000.0)
        };
        format!("{:>12}", s)
    }

    pub fn process_event(&mut self, event: &input_event) -> bool {
        let event_us = event_microseconds(event);

        // Track overall start and end times
        if self.overall_first_event_us.is_none() {
            self.overall_first_event_us = Some(event_us);
        }
        self.overall_last_event_us = Some(event_us);

        // if self.first_event_us.is_none() { // Removed
        //     self.first_event_us = Some(event_us);
        // }

        let is_key = is_key_event(event);
        let key_code = event.code;
        let key_value = event.value;

        // Get the timestamp of the last *passed* event for this specific key_code and value.
        // Handle out-of-bounds codes/values gracefully.
        let last_us_for_key_value = if (key_code as usize) < 1024 && (key_value as usize) < 3 {
            self.last_event_us[key_code as usize][key_value as usize]
        } else {
            u64::MAX // Treat out-of-bounds as if no event has passed
        };

        // Convert MAX to None, representing no previous passed event.
        let previous_last_passed_us = if last_us_for_key_value == u64::MAX {
            None
        } else {
            Some(last_us_for_key_value)
        };

        if self.log_all_events && self.last_event_was_syn {
            eprintln!(
                "{}",
                "--- Event Packet ---".on_bright_black().bold().underline().truecolor(255, 255, 0)
            );
        }

        let mut bounce_diff_us: Option<u64> = None;
        // Only debounce press (1) and release (0), NOT repeat (2)
        let is_bounce = if is_key && self.debounce_time_us > 0 && key_value != 2 {
            match previous_last_passed_us {
                Some(last_us) => {
                    if let Some(diff) = event_us.checked_sub(last_us) {
                        if diff < self.debounce_time_us {
                            bounce_diff_us = Some(diff);
                            true
                        } else {
                            // Check for near miss *before* returning false
                            if diff < 100_000 {
                                // Define the key tuple before using it
                                let key = (key_code, key_value);
                                self.stats.record_near_miss(key, diff);
                                self.interval_stats.record_near_miss(key, diff);
                            }
                            false // Event is not a bounce, pass it
                        }
                    } else {
                        false
                    }
                }
                None => false,
            }
        } else {
            false
        };

        if is_key {
            self.stats.record_event(key_code, key_value, is_bounce, bounce_diff_us, event_us);
            self.interval_stats.record_event(key_code, key_value, is_bounce, bounce_diff_us, event_us);
            if (key_code as usize) < 1024 {
                self.last_any_event_us[key_code as usize] = event_us;
                if !is_bounce && (key_value as usize) < 3 {
                    self.last_event_us[key_code as usize][key_value as usize] = event_us;
                }
            }
        }

        if self.log_all_events {
            self.log_event(event, event_us, is_bounce, bounce_diff_us, previous_last_passed_us);
        } else if self.log_bounces && is_bounce && is_key {
            self.log_simple_bounce(event, event_us, bounce_diff_us);
        }

        self.last_event_was_syn = event.type_ == EV_SYN as u16;

        if self.log_interval_us > 0 {
            // Use wallclock time for periodic logging, not event timestamp
            use std::time::{SystemTime, UNIX_EPOCH};
            let now_us = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_micros() as u64)
                .unwrap_or(event_us);

            let dump_needed = match self.last_stats_dump_time_us {
                Some(last_dump_us) => now_us.saturating_sub(last_dump_us) >= self.log_interval_us,
                None => true,
            };
            if dump_needed {
                eprintln!(
                    "\n{} {} {}",
                    "--- Periodic Stats Dump (Wallclock:".magenta().bold(),
                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string().on_bright_black().bright_yellow().bold(),
                    ") ---".magenta().bold()
                );
                // Use the stored stats_json flag instead of checking env::args
                if self.stats_json {
                    eprintln!("{}", "Cumulative stats:".on_bright_black().bold().bright_white());
                    // Calculate overall runtime for cumulative stats
                    let overall_runtime = self.overall_last_event_us.and_then(|last| {
                        self.overall_first_event_us.map(|first| last.saturating_sub(first))
                    });
                    self.stats.print_stats_json(
                        self.debounce_time_us,
                        self.log_all_events,
                        self.log_bounces,
                        self.log_interval_us,
                        overall_runtime, // Pass overall runtime
                        std::io::stderr(),
                    );
                    eprintln!("{}", "Interval stats (since last dump):".on_bright_black().bold().bright_white());
                    // Calculate interval runtime
                    let interval_start_us = self.last_stats_dump_time_us
                        .or(self.overall_first_event_us) // Fallback to overall start if first interval
                        .unwrap_or(now_us); // Fallback to now if no events yet
                    let interval_runtime = now_us.saturating_sub(interval_start_us);
                    self.interval_stats.print_stats_json(
                        self.debounce_time_us,
                        self.log_all_events,
                        self.log_bounces,
                        self.log_interval_us,
                        Some(interval_runtime), // Pass interval runtime
                        std::io::stderr(),
                    );
                }
                eprintln!("{}", "Cumulative stats:".on_bright_black().bold().bright_white());
                let _ = self.print_stats(&mut io::stderr());
                eprintln!("{}", "Interval stats (since last dump):".on_bright_black().bold().bright_white());
                self.interval_stats.print_stats_to_stderr(
                    self.debounce_time_us,
                    self.log_all_events,
                    self.log_bounces,
                    self.log_interval_us,
                );
                eprintln!("{}", "-------------------------------------------\n".magenta().bold());
                self.last_stats_dump_time_us = Some(now_us);
                // Reset interval stats - with_capacity takes no arguments
                self.interval_stats = StatsCollector::with_capacity();
            }
        }

        is_bounce
    }

    fn log_event(
        &self,
        event: &input_event,
        event_us: u64,
        is_bounce: bool,
        bounce_diff_us: Option<u64>,
        previous_last_passed_us: Option<u64>,
    ) {
        // Colorful status
        let status = if is_bounce {
            "[DROP]".on_red().white().bold()
        } else {
            "[PASS]".on_green().black().bold()
        };

        // Use the overall first event timestamp for relative logging
        let relative_us = event_us.saturating_sub(self.overall_first_event_us.unwrap_or(event_us));
        let relative_time_str = Self::format_timestamp_relative(relative_us)
            .on_bright_black()
            .bright_yellow()
            .bold();

        let type_name = get_event_type_name(event.type_)
            .on_bright_black()
            .bright_cyan()
            .bold();

        let mut event_details = String::new();
        let mut timing_info = String::new();

        if is_key_event(event) {
            let key_code = event.code;
            let key_value = event.value;
            let key_name = get_key_name(key_code)
                .on_bright_black()
                .bright_magenta()
                .bold();
            let code_str = format!("{}", key_code).bright_blue().bold();
            let value_str = format!("{}", key_value).bright_yellow().bold();
            event_details.push_str(&format!(
                "[{}] ({}, {})",
                key_name, code_str, value_str
            ));

            if is_bounce {
                if let Some(diff) = bounce_diff_us {
                    timing_info.push_str(&format!(
                        " {} {}",
                        "Bounce Diff:".on_bright_black().bright_red().bold(),
                        crate::filter::stats::format_us(diff)
                            .on_bright_black()
                            .bright_red()
                            .bold()
                    ));
                }
            } else if let Some(prev) = previous_last_passed_us {
                let time_since_last_passed = event_us.saturating_sub(prev);
                timing_info.push_str(&format!(
                    " {} {}",
                    "Time since last passed:".on_bright_black().bright_green().bold(),
                    crate::filter::stats::format_us(time_since_last_passed)
                        .on_bright_black()
                        .bright_green()
                        .bold()
                ));
            } else {
                timing_info.push_str(
                    ", "
                );
                timing_info.push_str(
                    &"First passed event of this type"
                        .on_bright_black()
                        .bright_cyan()
                        .bold()
                        .to_string()
                );
            }
        } else {
            let code_str = format!("{}", event.code).bright_blue().bold();
            let value_str = format!("{}", event.value).bright_yellow().bold();
            event_details.push_str(&format!(
                "Code: {}, Value: {}",
                code_str, value_str
            ));
        }

        let padded_details = format!("{:<30}", event_details)
            .on_bright_black()
            .white();

        let indentation = "  ";

        eprintln!(
            "{}{}{} {} ({}) {}{}",
            indentation,
            status,
            relative_time_str,
            type_name,
            event.type_,
            padded_details,
            timing_info
        );
    }

    fn log_simple_bounce(&self, event: &input_event, event_us: u64, bounce_diff_us: Option<u64>) {
        let code = event.code;
        let value = event.value;
        let type_name = get_event_type_name(event.type_)
            .on_bright_black()
            .bright_cyan()
            .bold();
        let key_name = get_key_name(code)
            .on_bright_black()
            .bright_magenta()
            .bold();
        let code_str = format!("{}", code).bright_blue().bold();
        let value_str = format!("{}", value).bright_yellow().bold();

        eprint!(
            "{} {} {} {}, Type: {} ({}), Code: {} [{}], Value: {}",
            "[DROP]".on_red().white().bold(),
            event_us.to_string().on_bright_black().bright_yellow().bold(),
            "µs".on_bright_black().bright_yellow().bold(),
            " ".on_bright_black(),
            type_name,
            event.type_,
            code_str,
            key_name,
            value_str
        );
        if let Some(diff) = bounce_diff_us {
            eprint!(
                ", {} {}",
                "Bounce Diff:".on_bright_black().bright_red().bold(),
                crate::filter::stats::format_us(diff)
                    .on_bright_black()
                    .bright_red()
                    .bold()
            );
        }
        eprintln!();
    }

    pub fn print_stats(&self, _writer: &mut impl Write) -> io::Result<()> {
        // Calculate and print total runtime using overall timestamps
        if let (Some(first), Some(last)) = (self.overall_first_event_us, self.overall_last_event_us) {
            let duration = last.saturating_sub(first);
            // Print runtime before the rest of the stats for better structure
             eprintln!(
                 "{} {}",
                 "Total runtime:".on_bright_black().bold().bright_yellow(),
                 stats::format_us(duration).on_bright_black().bright_yellow().bold()
             );
        } else {
             eprintln!(
                 "{} {}",
                 "Total runtime:".on_bright_black().bold().bright_yellow(),
                 "No events processed".on_bright_black().dimmed()
             );
        }

        // Now print the rest of the stats (which no longer includes runtime)
        self.stats.print_stats_to_stderr(
            self.debounce_time_us,
            self.log_all_events,
            self.log_bounces,
            self.log_interval_us,
        );
        Ok(())
    }
}
