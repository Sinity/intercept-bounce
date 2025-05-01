mod keynames;
pub mod stats;

use crate::event::{event_microseconds, is_key_event};
use input_linux_sys::{input_event, EV_SYN};
use std::collections::HashMap;
use std::io::{self, Write};
use colored::*;

use keynames::{get_key_name, get_event_type_name};
use stats::StatsCollector;

/// Holds the state for bounce filtering.
pub struct BounceFilter {
    pub debounce_time_us: u64,
    pub log_interval_us: u64,
    pub log_all_events: bool,
    pub log_bounces: bool,
    last_event_us: HashMap<(u16, i32), u64>,
    last_any_event_us: HashMap<u16, u64>,
    first_event_us: Option<u64>,
    last_event_was_syn: bool,
    pub stats: StatsCollector,
    last_stats_dump_time_us: Option<u64>,
}

impl BounceFilter {
    pub fn new(debounce_time_ms: u64, log_interval_s: u64, log_all_events: bool, log_bounces: bool) -> Self {
        // Generous memory: pre-allocate large hashmaps and vectors
        BounceFilter {
            debounce_time_us: debounce_time_ms * 1_000,
            log_interval_us: log_interval_s * 1_000_000,
            log_all_events,
            log_bounces,
            last_event_us: HashMap::with_capacity(4096),
            last_any_event_us: HashMap::with_capacity(4096),
            first_event_us: None,
            last_event_was_syn: true,
            stats: StatsCollector::with_capacity(4096),
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

        if self.first_event_us.is_none() {
            self.first_event_us = Some(event_us);
        }

        let is_key = is_key_event(event);
        let key_code = event.code;
        let key_value = event.value;
        let key = (key_code, key_value);
        let previous_last_passed_us = self.last_event_us.get(&key).copied();

        if self.log_all_events && self.last_event_was_syn {
            eprintln!("{}", "--- Event Packet ---".blue().bold());
        }

        let mut bounce_diff_us: Option<u64> = None;
        let is_bounce = if is_key && self.debounce_time_us > 0 {
            match previous_last_passed_us {
                Some(last_us) => {
                    if let Some(diff) = event_us.checked_sub(last_us) {
                        if diff < self.debounce_time_us {
                            bounce_diff_us = Some(diff);
                            true
                        } else {
                            if diff < 100_000 {
                                self.stats.record_near_miss(key, diff);
                            }
                            false
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
            let _ = self.last_any_event_us.insert(key_code, event_us);
            if !is_bounce {
                self.last_event_us.insert(key, event_us);
            }
        }

        if self.log_all_events {
            self.log_event(event, event_us, is_bounce, bounce_diff_us, previous_last_passed_us);
        } else if self.log_bounces && is_bounce && is_key {
            self.log_simple_bounce(event, event_us, bounce_diff_us);
        }

        self.last_event_was_syn = event.type_ == EV_SYN as u16;

        if self.log_interval_us > 0 {
            let dump_needed = match self.last_stats_dump_time_us {
                Some(last_dump_us) => event_us.saturating_sub(last_dump_us) >= self.log_interval_us,
                None => true,
            };
            if dump_needed {
                eprintln!(
                    "\n{} {} {}",
                    "--- Periodic Stats Dump (Time:".magenta().bold(),
                    event_us,
                    "µs) ---".magenta().bold()
                );
                if std::env::args().any(|a| a == "--stats-json") {
                    self.stats.print_stats_json(
                        self.debounce_time_us,
                        self.log_all_events,
                        self.log_bounces,
                        self.log_interval_us,
                        std::io::stderr(),
                    );
                }
                let _ = self.print_stats(&mut io::stderr());
                eprintln!("{}", "-------------------------------------------\n".magenta().bold());
                self.last_stats_dump_time_us = Some(event_us);
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
        let status = if is_bounce {
            "[DROP]".red().bold()
        } else {
            "[PASS]".green().bold()
        };
        let relative_us = event_us.saturating_sub(self.first_event_us.unwrap_or(event_us));
        let relative_time_str = Self::format_timestamp_relative(relative_us);
        let type_name = get_event_type_name(event.type_);

        let mut event_details = String::new();
        let mut timing_info = String::new();

        if is_key_event(event) {
            let key_code = event.code;
            let key_value = event.value;
            let key_name = get_key_name(key_code);
            event_details.push_str(&format!("[{}] ({}, {})", key_name, key_code, key_value));

            if is_bounce {
                if let Some(diff) = bounce_diff_us {
                    timing_info.push_str(&format!(" Bounce Diff: {}", crate::filter::stats::format_us(diff)));
                }
            } else if let Some(prev) = previous_last_passed_us {
                let time_since_last_passed = event_us.saturating_sub(prev);
                timing_info.push_str(&format!(" Time since last passed: {}", crate::filter::stats::format_us(time_since_last_passed)));
            } else {
                timing_info.push_str(", First passed event of this type");
            }
        } else {
            event_details.push_str(&format!("Code: {}, Value: {}", event.code, event.value));
        }

        let padded_details = format!("{:<30}", event_details);
        let indentation = "  ";

        eprintln!(
            "{}{}{} {} ({}) {}{}",
            indentation,
            status,
            relative_time_str,
            type_name.cyan(),
            event.type_,
            padded_details,
            timing_info
        );
    }

    fn log_simple_bounce(&self, event: &input_event, event_us: u64, bounce_diff_us: Option<u64>) {
        let code = event.code;
        let value = event.value;
        let type_name = get_event_type_name(event.type_);
        let key_name = get_key_name(code);

        eprint!(
            "{} {} µs, Type: {} ({}), Code: {} [{}], Value: {}",
            "[DROP]".red().bold(),
            event_us,
            type_name.cyan(),
            event.type_,
            code,
            key_name,
            value
        );
        if let Some(diff) = bounce_diff_us {
            eprint!(", Bounce Diff: {}", crate::filter::stats::format_us(diff));
        }
        eprintln!();
    }

    pub fn print_stats(&self, _writer: &mut impl Write) -> io::Result<()> {
        self.stats.print_stats_to_stderr(
            self.debounce_time_us,
            self.log_all_events,
            self.log_bounces,
            self.log_interval_us,
        );
        Ok(())
    }
}
