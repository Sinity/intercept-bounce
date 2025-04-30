use crate::event::{event_microseconds, is_key_event};
use input_linux_sys::{input_event, EV_ABS, EV_KEY, EV_LED, EV_MSC, EV_REL, EV_SYN};
use std::collections::HashMap;
use std::io::{self, Write};

// Include the generated static map for key names
// Source: /usr/include/linux/input-event-codes.h
// (Add more keys as needed for better logging)
static KEY_NAMES: phf::Map<u16, &'static str> = phf::phf_map! {
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
    71u16 => "KEY_KP7", // Corrected typo 71u116 -> 71u16
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

    // --- Statistics ---
    key_events_processed: u64,
    key_events_passed: u64,
    key_events_dropped: u64,
    per_key_stats: HashMap<u16, KeyStats>, // Stores drop counts and bounce timings
    per_key_passed_near_miss_timing: HashMap<(u16, i32), Vec<u64>>, // Timings for passed events < 100ms
    last_stats_dump_time_us: Option<u64>, // For time-based periodic logging
}


impl BounceFilter {
    /// Creates a new BounceFilter.
    pub fn new(debounce_time_ms: u64, log_interval_s: u64, log_all_events: bool, log_bounces: bool) -> Self {
        BounceFilter {
            debounce_time_us: debounce_time_ms * 1_000,
            log_interval_us: log_interval_s * 1_000_000, // Convert seconds to microseconds
            log_all_events,
            log_bounces,
            last_event_us: HashMap::with_capacity(64),
            last_any_event_us: HashMap::with_capacity(64),
            key_events_processed: 0,
            key_events_passed: 0,
            key_events_dropped: 0,
            per_key_stats: HashMap::with_capacity(64),
            per_key_passed_near_miss_timing: HashMap::with_capacity(64), // Init near-miss map
            last_stats_dump_time_us: None, // Initialize periodic dump time
        }
    }

    /// Gets the human-readable name for a key code, or the code itself if unknown.
    fn get_key_name(code: u16) -> String {
        KEY_NAMES.get(&code).map_or_else(|| code.to_string(), |name| name.to_string())
    }

    /// Gets the human-readable name for an event type.
    fn get_event_type_name(type_: u16) -> &'static str {
        match i32::from(type_) {
            EV_SYN => "EV_SYN",
            EV_KEY => "EV_KEY",
            EV_REL => "EV_REL",
            EV_ABS => "EV_ABS",
            EV_MSC => "EV_MSC", // Added
            EV_LED => "EV_LED", // Added
            // Add other types as needed
            _ => "Unknown", // Return a static string for unknown types
        }
    }

    /// Formats microseconds into a human-readable string (ms or µs).
    fn format_us(us: u64) -> String {
        if us >= 1000 {
            format!("{:.1} ms", us as f64 / 1000.0)
        } else {
            format!("{} µs", us)
        }
    }


    /// Processes an incoming event.
    /// Logs details if `log_events` is true.
    /// Checks for bounce if not in `bypass` mode and it's a key event.
    /// Updates internal state and statistics.
    /// Returns `true` if the event was considered a bounce and should be dropped, `false` otherwise.
    pub fn process_event(&mut self, event: &input_event) -> bool {
        let event_us = event_microseconds(event); // Get timestamp once

        // 1. Only apply bounce filtering logic to key events
        if !is_key_event(event) {
            // Log non-key event if requested
            if self.log_all_events {
                self.log_event_details(event, event_us, false, None); // false = not a bounce
            }
            return false; // Not a key event, never a bounce
        }

        // Now we know it's a key event, proceed with bounce check and state update
        self.key_events_processed += 1; // Count all incoming key events

        let key_code = event.code;
        let key_value = event.value;
        let key = (key_code, key_value);

        // 2. Check for bounce based on *current* state
        let mut bounce_diff_us: Option<u64> = None;
        let is_bounce = if self.debounce_time_us == 0 {
            false // Debounce time 0 means no filtering
        } else {
            match self.last_event_us.get(&key) {
                Some(&last_us) => {
                    if let Some(diff) = event_us.checked_sub(last_us) {
                        if diff < self.debounce_time_us {
                            bounce_diff_us = Some(diff);
                            true // It's a bounce
                        } else {
                            // Event is outside debounce time, but check for near-miss
                            if diff < 100_000 { // Less than 100ms
                                // Store as near-miss timing
                                self.per_key_passed_near_miss_timing.entry(key).or_default().push(diff);
                            }
                            false // Not a bounce
                        }
                    } else {
                        false // Not a bounce (time went backwards)
                    }
                }
                None => {
                    false // First event for this key/value pair
                }
            }
        };

        // 3. Update state and statistics *after* checking bounce status
        let _ = self.last_any_event_us.insert(key_code, event_us); // Update last *any* time

        if is_bounce {
            // --- Event was a bounce ---
            self.key_events_dropped += 1;
            let key_stats = self.per_key_stats.entry(key_code).or_default();
            let value_stats = match key_value {
                1 => &mut key_stats.press,
                0 => &mut key_stats.release,
                _ => &mut key_stats.repeat,
            };
            value_stats.count += 1;
            // Always store detailed timings now
            if let Some(diff) = bounce_diff_us {
                value_stats.timings_us.push(diff);
            }
            if self.log_all_events {
                self.log_event_details(event, event_us, true, bounce_diff_us);
            } else if self.log_bounces {
                self.log_bounce_details(event, event_us, bounce_diff_us);
            }

        } else {
            // --- Event was NOT a bounce ---
            self.key_events_passed += 1;
            // Update the last timestamp for this specific key+value state
            self.last_event_us.insert(key, event_us);

            // Log if requested
            if self.log_all_events {
                self.log_event_details(event, event_us, false, None);
            }

            // Note: Repeat logging based on last_any_event_us is removed for simplicity,
            // near-miss stats provide similar timing insights for passed events.
        }

        // 4. Dump stats periodically based on time interval
        if self.log_interval_us > 0 {
            let dump_needed = match self.last_stats_dump_time_us {
                Some(last_dump_us) => event_us.saturating_sub(last_dump_us) >= self.log_interval_us,
                None => true, // Dump stats the first time if interval is set
            };
            if dump_needed {
                 eprintln!("\n--- Periodic Stats Dump (Time: {} µs) ---", event_us);
                 let _ = self.print_stats(&mut io::stderr()); // Ignore errors
                 eprintln!("-------------------------------------------\n");
                 self.last_stats_dump_time_us = Some(event_us); // Update last dump time
            }
        }

        // 5. Return bounce status
        is_bounce
    }

    fn log_event_details(&self, event: &input_event, event_us: u64, is_bounce: bool, bounce_diff_us: Option<u64>) {
        let status = if is_bounce { "[DROP]" } else { "[PASS]" };
        let code = event.code;
        let value = event.value;
        let type_name = Self::get_event_type_name(event.type_);

        eprint!("{:<6} Timestamp: {} µs, Type: {} ({}), Code: {}, Value: {}",
            status,
            event_us,
            event.type_, // Log the raw type number
            type_name,   // Log the resolved name
            code,
            value
        );

        if is_key_event(event) { // Use the imported helper
            let key = (code, value);
            let key_name = Self::get_key_name(code);
            eprint!(" [{}]", key_name);

            // Show timing information relevant to the event's status
            if let Some(&_last_us) = self.last_event_us.get(&key) { // Use _last_us to silence warning
                 // For dropped events, last_us wasn't updated, so this diff is the bounce diff
                 // For passed events, last_us *was* updated, so recalculate diff relative to *previous* state if needed for logging?
                 // Let's just show the bounce diff if it was a bounce, otherwise show time since last *passed* event of this type.
                 if is_bounce {
                     if let Some(diff) = bounce_diff_us {
                         eprint!(", Bounce Diff: {}", Self::format_us(diff));
                     }
                 } else {
                     // This event passed. Show time since the *previous* passed event for this key/value.
                     // Show time since last *any* event for this key code
                     if let Some(&last_any_us) = self.last_any_event_us.get(&code) {
                         if let Some(diff) = event_us.checked_sub(last_any_us) {
                              eprint!(", Time since last any: {}", Self::format_us(diff));
                         }
                     }
                 }
            } else {
                 eprint!(", First event for ({}, {})", code, value);
            }
        }
        eprintln!();
    }

    fn log_bounce_details(&self, event: &input_event, event_us: u64, bounce_diff_us: Option<u64>) {
        let code = event.code;
        let value = event.value;
        let type_name = Self::get_event_type_name(event.type_);
        let key_name = Self::get_key_name(code);

        eprint!("[DROP] Timestamp: {} µs, Type: {} ({}), Code: {} [{}], Value: {}",
            event_us,
            event.type_, type_name, code, key_name, value
        );
        if let Some(diff) = bounce_diff_us {
            eprint!(", Bounce Diff: {}", Self::format_us(diff));
        }
        eprintln!();
    }


    /// Prints collected statistics to stderr.
    pub fn print_stats(&self, _writer: &mut impl Write) -> io::Result<()> {
        eprintln!("--- intercept-bounce status ---");
        eprintln!("Debounce Threshold: {}", Self::format_us(self.debounce_time_us)); // Renamed field
        eprintln!("Log All Events (--log-all-events): {}", if self.log_all_events { "Active" } else { "Inactive" });
        eprintln!("Log Bounces (--log-bounces): {}", if self.log_bounces { "Active" } else { "Inactive" });
        // Removed 'Collect Detailed Stats' line
        eprintln!("Periodic Log Interval (--log-interval): {}", if self.log_interval_us > 0 { format!("Every {} seconds", self.log_interval_us / 1_000_000) } else { "Disabled".to_string() });

        eprintln!("\n--- Overall Statistics ---");
        eprintln!("Key Events Processed: {}", self.key_events_processed);
        eprintln!("Key Events Passed:    {}", self.key_events_passed);
        eprintln!("Key Events Dropped:   {}", self.key_events_dropped);
        let percentage = if self.key_events_processed > 0 {
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
        } else {
            0.0
        };
        eprintln!("Percentage Dropped:   {:.2}%", percentage);

        if !self.per_key_stats.is_empty() {
            eprintln!("\n--- Dropped Event Statistics Per Key ---");
            // Always print detailed format now
            eprintln!("Format: Key [Name] (Code):");
            eprintln!("  State (Value): Drop Count (Bounce Time: Min / Avg / Max)");

            let mut sorted_keys: Vec<_> = self.per_key_stats.keys().collect();
            sorted_keys.sort();

            for key_code in sorted_keys {
                if let Some(stats) = self.per_key_stats.get(key_code) {
                    let key_name = Self::get_key_name(*key_code);
                    let total_drops_for_key = stats.press.count + stats.release.count + stats.repeat.count;

                    if total_drops_for_key > 0 {
                        eprintln!("\nKey [{}] ({}):", key_name, key_code);

                        // Helper closure to print stats for a specific value
                        let print_value_stats = |value_name: &str, value_code: i32, value_stats: &KeyValueStats| {
                            if value_stats.count > 0 {
                                eprint!("  {:<7} ({}): {}", value_name, value_code, value_stats.count);
                                // Always try to print timings now
                                if !value_stats.timings_us.is_empty() {
                                    let timings = &value_stats.timings_us;
                                    let min = timings.iter().min().unwrap_or(&0); // Should not happen if count > 0
                                    let max = timings.iter().max().unwrap_or(&0); // Should not happen if count > 0
                                    let sum: u64 = timings.iter().sum();
                                    let avg = sum as f64 / timings.len() as f64;
                                    eprintln!(" (Bounce Time: {} / {} / {})",
                                        Self::format_us(*min),
                                        Self::format_us(avg as u64), // Format avg too
                                        Self::format_us(*max)
                                    );
                                } else {
                                    eprintln!(" (No timing data collected)");
                                }
                            }
                        };

                        print_value_stats("Press", 1, &stats.press);
                        print_value_stats("Release", 0, &stats.release);
                        print_value_stats("Repeat", 2, &stats.repeat);
                    }
                }
            }
        } else {
            eprintln!("\n--- No key events dropped ---");
        }

        // --- Near Miss Stats ---
        if !self.per_key_passed_near_miss_timing.is_empty() {
             eprintln!("\n--- Passed Event Near-Miss Statistics (Passed within 100ms) ---");
             eprintln!("Format: Key [Name] (Code, Value): Count (Timings: Min / Avg / Max)");

             let mut sorted_near_misses: Vec<_> = self.per_key_passed_near_miss_timing.iter().collect();
             sorted_near_misses.sort_by_key(|(k, _)| *k); // Sort by (code, value)

             for ((code, value), timings) in sorted_near_misses {
                 if !timings.is_empty() {
                     let key_name = Self::get_key_name(*code);
                     let min = timings.iter().min().unwrap_or(&0);
                     let max = timings.iter().max().unwrap_or(&0);
                     let sum: u64 = timings.iter().sum();
                     let avg = sum as f64 / timings.len() as f64;
                     eprintln!("  Key [{}] ({}, {}): {} (Timings: {} / {} / {})",
                         key_name, code, value, timings.len(),
                         Self::format_us(*min),
                         Self::format_us(avg as u64),
                         Self::format_us(*max)
                     );
                 }
             }
        } else {
             eprintln!("\n--- No near-miss events recorded (< 100ms) ---");
        }


        eprintln!("----------------------------------------------------------");
        Ok(())
    }
}
