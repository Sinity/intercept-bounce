mod keynames;
mod stats;

use crate::event::{event_microseconds, is_key_event};
use input_linux_sys::{input_event, EV_ABS, EV_KEY, EV_LED, EV_MSC, EV_REL, EV_SYN};
use std::collections::HashMap;
use std::io::{self, Write};

use keynames::{get_key_name, get_event_type_name};
use stats::{KeyStats, KeyValueStats, StatsCollector};
    0u16 => "KEY_RESERVED",
    1u16 => "KEY_ESC",
    2u16 => "KEY_1",
    3u16 => "KEY_2",
    4u16 => "KEY_3",
    5u16 => "KEY_4",
    6u16 => "KEY_5",
    7u16 => "KEY_6",
    8u16 => "KEY_7",
    9u16 => "KEY_8",
    10u16 => "KEY_9",
    11u16 => "KEY_0",
    12u16 => "KEY_MINUS",
    13u16 => "KEY_EQUAL",
    14u16 => "KEY_BACKSPACE",
    15u16 => "KEY_TAB",
    16u16 => "KEY_Q",
    17u16 => "KEY_W",
    18u16 => "KEY_E",
    19u16 => "KEY_R",
    20u16 => "KEY_T",
    21u16 => "KEY_Y",
    22u16 => "KEY_U",
    23u16 => "KEY_I",
    24u16 => "KEY_O",
    25u16 => "KEY_P",
    26u16 => "KEY_LEFTBRACE",
    27u16 => "KEY_RIGHTBRACE",
    28u16 => "KEY_ENTER",
    29u16 => "KEY_LEFTCTRL",
    30u16 => "KEY_A",
    31u16 => "KEY_S",
    32u16 => "KEY_D",
    33u16 => "KEY_F",
    34u16 => "KEY_G",
    35u16 => "KEY_H",
    36u16 => "KEY_J",
    37u16 => "KEY_K",
    38u16 => "KEY_L",
    39u16 => "KEY_SEMICOLON",
    40u16 => "KEY_APOSTROPHE",
    41u16 => "KEY_GRAVE",
    42u16 => "KEY_LEFTSHIFT",
    43u16 => "KEY_BACKSLASH",
    44u16 => "KEY_Z",
    45u16 => "KEY_X",
    46u16 => "KEY_C",
    47u16 => "KEY_V",
    48u16 => "KEY_B",
    49u16 => "KEY_N",
    50u16 => "KEY_M",
    51u16 => "KEY_COMMA",
    52u16 => "KEY_DOT",
    53u16 => "KEY_SLASH",
    54u16 => "KEY_RIGHTSHIFT",
    55u16 => "KEY_KPASTERISK",
    56u16 => "KEY_LEFTALT",
    57u16 => "KEY_SPACE",
    58u16 => "KEY_CAPSLOCK",
    // --- Add common keys ---
    59u16 => "KEY_F1",
    60u16 => "KEY_F2",
    61u16 => "KEY_F3",
    62u16 => "KEY_F4",
    63u16 => "KEY_F5",
    64u16 => "KEY_F6",
    65u16 => "KEY_F7",
    66u16 => "KEY_F8",
    67u16 => "KEY_F9",
    68u16 => "KEY_F10",
    69u16 => "KEY_NUMLOCK",
    70u16 => "KEY_SCROLLLOCK",
    71u16 => "KEY_KP7",
    72u16 => "KEY_KP8",
    73u16 => "KEY_KP9",
    74u16 => "KEY_KPMINUS",
    75u16 => "KEY_KP4",
    76u16 => "KEY_KP5",
    77u16 => "KEY_KP6",
    78u16 => "KEY_KPPLUS",
    79u16 => "KEY_KP1",
    80u16 => "KEY_KP2",
    81u16 => "KEY_KP3",
    82u16 => "KEY_KP0",
    83u16 => "KEY_KPDOT",
    87u16 => "KEY_F11",
    88u16 => "KEY_F12",
    96u16 => "KEY_KPENTER",
    97u16 => "KEY_RIGHTCTRL",
    98u16 => "KEY_KPSLASH",
    99u16 => "KEY_SYSRQ",
    100u16 => "KEY_RIGHTALT",
    102u16 => "KEY_HOME",
    103u16 => "KEY_UP",
    104u16 => "KEY_PAGEUP",
    105u16 => "KEY_LEFT",
    106u16 => "KEY_RIGHT",
    107u16 => "KEY_END",
    108u16 => "KEY_DOWN",
    109u16 => "KEY_PAGEDOWN",
    110u16 => "KEY_INSERT",
    111u16 => "KEY_DELETE",
    119u16 => "KEY_PAUSE",
    125u16 => "KEY_LEFTMETA", // Windows/Super key
    126u16 => "KEY_RIGHTMETA",
    127u16 => "KEY_COMPOSE", // Menu key
};

// --- Statistics Structs ---
/// Stores statistics for a specific key value (press/release/repeat).
#[derive(Default, Debug)]
struct KeyValueStats {
    count: u64,
    timings_us: Vec<u64>, // Time diffs that caused a bounce for this value
}

/// Stores aggregated statistics for a specific key code.
#[derive(Default, Debug)]
struct KeyStats {
    press: KeyValueStats,   // value 1
    release: KeyValueStats, // value 0
    repeat: KeyValueStats,  // value 2
}
// --- End Statistics Structs ---

/// Holds the state for bounce filtering.
pub struct BounceFilter {
    debounce_time_us: u64, // Renamed from window_us
    log_interval_us: u64,  // Now in microseconds (0 = disabled)
    log_all_events: bool,
    log_bounces: bool,
    last_event_us: HashMap<(u16, i32), u64>, // Map (key code, value) -> last passed event timestamp (µs)
    last_any_event_us: HashMap<u16, u64>,    // Map key code -> last processed event timestamp (µs)
    first_event_us: Option<u64>, // Timestamp of the very first event processed
    last_event_was_syn: bool, // Track if the previous event was EV_SYN for logging groups
    stats: StatsCollector,
    last_stats_dump_time_us: Option<u64>, // For time-based periodic logging
}

impl BounceFilter {
    pub fn new(debounce_time_ms: u64, log_interval_s: u64, log_all_events: bool, log_bounces: bool) -> Self {
        BounceFilter {
            debounce_time_us: debounce_time_ms * 1_000,
            log_interval_us: log_interval_s * 1_000_000,
            log_all_events,
            log_bounces,
            last_event_us: HashMap::with_capacity(64),
            last_any_event_us: HashMap::with_capacity(64),
            first_event_us: None,
            last_event_was_syn: true,
            stats: StatsCollector::new(),
            last_stats_dump_time_us: None,
        }
    }

    fn format_us(us: u64) -> String {
        if us >= 1000 {
            format!("{:.1} ms", us as f64 / 1000.0)
        } else {
            format!("{} µs", us)
        }
    }

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
            eprintln!("--- Event Packet ---");
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
            self.stats.record_event(key_code, key_value, is_bounce, bounce_diff_us);
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
                eprintln!("\n--- Periodic Stats Dump (Time: {} µs) ---", event_us);
                let _ = self.print_stats(&mut io::stderr());
                eprintln!("-------------------------------------------\n");
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
        let status = if is_bounce { "[DROP]" } else { "[PASS]" };
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
                    timing_info.push_str(&format!(" Bounce Diff: {}", Self::format_us(diff)));
                }
            } else {
                if let Some(prev) = previous_last_passed_us {
                    let time_since_last_passed = event_us.saturating_sub(prev);
                    timing_info.push_str(&format!(" Time since last passed: {}", Self::format_us(time_since_last_passed)));
                } else {
                    timing_info.push_str(", First passed event of this type");
                }
            }
        } else {
            event_details.push_str(&format!("Code: {}, Value: {}", event.code, event.value));
        }

        let padded_details = format!("{:<30}", event_details);
        let indentation = if event.type_ == EV_SYN as u16 { "" } else { "  " };

        eprintln!(
            "{}{}{} {} ({}) {}{}",
            indentation, status, relative_time_str, type_name, event.type_, padded_details, timing_info
        );
    }

    fn log_simple_bounce(&self, event: &input_event, event_us: u64, bounce_diff_us: Option<u64>) {
        let code = event.code;
        let value = event.value;
        let type_name = get_event_type_name(event.type_);
        let key_name = get_key_name(code);

        eprint!(
            "[DROP] Timestamp: {} µs, Type: {} ({}), Code: {} [{}], Value: {}",
            event_us, event.type_, type_name, code, key_name, value
        );
        if let Some(diff) = bounce_diff_us {
            eprint!(", Bounce Diff: {}", Self::format_us(diff));
        }
        eprintln!();
    }

    pub fn print_stats(&self, _writer: &mut impl Write) -> io::Result<()> {
        self.stats.print_stats(
            self.debounce_time_us,
            self.log_all_events,
            self.log_bounces,
            self.log_interval_us,
        );
        Ok(())
    }
}
