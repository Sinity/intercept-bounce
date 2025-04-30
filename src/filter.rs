use input_linux_sys::{input_event, EV_KEY, EV_SYN, EV_REL, EV_ABS}; // Added other common types for logging
use phf;
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
    71u116 => "KEY_KP7",
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

/// Holds the state for bounce filtering, tracking the last event time for each key code and value (press/release/repeat state).
pub struct BounceFilter {
    window_us: u64,
    is_verbose: bool,
    log_interval: u64,
    bypass: bool,      // Bypass filtering
    log_events: bool,  // Log every event detail
    last_event_us: HashMap<(u16, i32), u64>, // Map (key code, value) -> last event timestamp (µs) for bounce check
    last_any_event_us: HashMap<u16, u64>,    // Map key code -> last event timestamp (µs) for repeat logging

    // --- Statistics (only updated if verbose AND not in bypass) ---
    key_events_processed: u64,
    key_events_dropped: u64,
    per_key_dropped: HashMap<(u16, i32), u64>, // Map (key code, value) -> drop count
    per_key_timing: HashMap<(u16, i32), Vec<u64>>, // Map (key code, value) -> Vec of time diffs (µs) for dropped events
}


impl BounceFilter {
    /// Creates a new BounceFilter.
    /// `window_ms`: The time window in milliseconds. Events within this window are filtered.
    /// `is_verbose`: Enables statistics collection and logging.
    /// `log_interval`: If > 0 and `is_verbose`, dumps stats every N key events.
    /// `bypass`: If true, all events are passed through without filtering.
    /// `log_events`: If true, logs details of every event to stderr.
    pub fn new(window_ms: u64, is_verbose: bool, log_interval: u64, bypass: bool, log_events: bool) -> Self {
        BounceFilter {
            window_us: window_ms * 1_000,
            is_verbose,
            log_interval,
            bypass,
            log_events, // Store the log_events state
            last_event_us: HashMap::with_capacity(64),
            last_any_event_us: HashMap::with_capacity(64), // Track last event for any value
            key_events_processed: 0,
            key_events_dropped: 0,
            per_key_dropped: HashMap::with_capacity(64),
            per_key_timing: HashMap::with_capacity(64), // Store timing diffs
        }
    }

    /// Gets the human-readable name for a key code, or the code itself if unknown.
    fn get_key_name(code: u16) -> String {
        KEY_NAMES.get(&code).map_or_else(|| code.to_string(), |name| name.to_string())
    }

    /// Gets the human-readable name for an event type, or the type itself if unknown.
    fn get_event_type_name(type_: u16) -> String {
        match i32::from(type_) {
            EV_SYN => "EV_SYN".to_string(),
            EV_KEY => "EV_KEY".to_string(),
            EV_REL => "EV_REL".to_string(),
            EV_ABS => "EV_ABS".to_string(),
            // Add other types as needed
            _ => format!("{}", type_),
        }
    }


    /// Processes an incoming event.
    /// Logs details if `log_events` is true.
    /// Checks for bounce if not in `bypass` mode and it's a key event.
    /// Updates internal state and statistics.
    /// Returns `true` if the event was considered a bounce and should be dropped, `false` otherwise.
    pub fn process_event(&mut self, event: &input_event) -> bool {
        let event_us = event_microseconds(event); // Get timestamp once

        // 1. Log event details *before* state update/filtering if log_events is enabled
        if self.log_events {
            self.log_event_details(event, event_us);
        }

        // 2. If in bypass mode, always pass through (not a bounce)
        if self.bypass {
            return false; // Not a bounce
        }

        // 3. Only apply bounce filtering logic to key events
        if !is_key_event(event) {
             return false; // Not a key event, never a bounce
        }

        // Now we know it's a key event and not in bypass mode, proceed with bounce check and state update

        let key_code = event.code;
        let key_value = event.value;
        let key = (key_code, key_value);

        // 4. Check for bounce based on *current* state (before updating state for this event)
        let is_bounce = match self.last_event_us.get(&key) {
            Some(&last_us) => {
                // Check if the time difference is within the bounce window.
                // Use checked_sub to handle potential time jumps backwards gracefully (treat as not a bounce).
                event_us.checked_sub(last_us).map_or(false, |diff| {
                    diff < self.window_us
                })
            }
            None => {
                // First event for this key code + value combination, never a bounce.
                false
            }
        };

        // 5. Update state and statistics *after* checking bounce status
        // Update last seen time for *any* event on this key code (for repeat logging/timing)
        // This happens regardless of whether it's a bounce or not.
        self.last_any_event_us.insert(key_code, event_us);

        if !is_bounce {
            // If not a bounce, update the last timestamp for this specific key+value state
            self.last_event_us.insert(key, event_us);
        }


        // 6. Update verbose stats if enabled and it was a key event (processed)
        if self.is_verbose {
            self.key_events_processed += 1; // Count processed key events

            if is_bounce {
                // It was a bounce, update drop stats
                self.key_events_dropped += 1;
                *self.per_key_dropped.entry(key).or_insert(0) += 1;
                // Calculate and store bounce timing diff using the *old* last_us
                if let Some(&last_us) = self.last_event_us.get(&key) { // This gets the *new* last_us if not bounce, *old* if bounce
                     // We need the diff relative to the *previous* event.
                     // The `is_bounce` check already calculated this diff.
                     // Let's recalculate it here for clarity or store it earlier.
                     // Recalculating is fine as it's only in verbose path.
                     if let Some(&prev_us) = self.last_event_us.get(&key) { // Get the timestamp *before* this event
                         if let Some(diff) = event_us.checked_sub(prev_us) {
                             self.per_key_timing.entry(key).or_default().push(diff);
                         }
                     }
                }
            }

            // Extended Repeat Logging (only if verbose) - This was already handled in the old is_bounce,
            // but now it's part of the general processing flow. Let's move it here.
            if key_value == 2 { // Check if it's a repeat event
                if let Some(&last_any_us) = self.last_any_event_us.get(&key_code) {
                    // Use a slightly larger window for repeat logging to see typical repeat rates
                    let repeat_check_window_us = self.window_us.max(100_000); // max(window, 100ms)
                    if let Some(diff) = event_us.checked_sub(last_any_us) {
                        if diff < repeat_check_window_us {
                             // Log repeats within the extended window, even if not dropped by bounce filter
                             eprint!(
                                "[VERBOSE] Repeat: Key {} ({}), Value: {}, Time since last any: {} µs\n",
                                Self::get_key_name(key_code), key_code, key_value, diff
                            );
                        }
                    }
                }
            }

            // Dump stats periodically if log_interval is set
            if self.log_interval > 0 && self.key_events_processed % self.log_interval == 0 {
                 eprintln!("\n--- Periodic Stats Dump (Event {}) ---", self.key_events_processed);
                 // Ignore errors writing periodic stats
                 let _ = self.print_stats(&mut io::stderr());
                 eprintln!("---------------------------------------\n");
            }
        }
        // --- End Statistics Update ---


        // 7. Return bounce status
        is_bounce
    }

    /// Helper to log event details to stderr if log_events is true.
    /// Called *before* filtering logic updates state.
    fn log_event_details(&self, event: &input_event, event_us: u64) {
        let event_type = i32::from(event.type_);
        let code = event.code;
        let value = event.value;

        eprint!("[LOG] Timestamp: {} µs, Type: {} ({}), Code: {}, Value: {}",
            event_us,
            event_type,
            Self::get_event_type_name(event.type_),
            code,
            value
        );

        if event_type == EV_KEY {
            let key = (code, value);
            let key_name = Self::get_key_name(code);
            eprint!(" [{}]", key_name);

            // Time since last event of same key+value state
            if let Some(&last_us) = self.last_event_us.get(&key) {
                if let Some(diff) = event_us.checked_sub(last_us) {
                    eprint!(", Time since last ({}, {}): {} µs", code, value, diff);
                } else {
                    eprint!(", Time since last ({}, {}): Time went backwards", code, value);
                }
            } else {
                 eprint!(", Time since last ({}, {}): First event", code, value);
            }

            // Time since last *any* event for this key code
             if let Some(&last_any_us) = self.last_any_event_us.get(&code) {
                if let Some(diff) = event_us.checked_sub(last_any_us) {
                    eprint!(", Time since last any ({}): {} µs", code, diff);
                } else {
                    eprint!(", Time since last any ({}): Time went backwards", code);
                }
            } else {
                 eprint!(", Time since last any ({}): First event", code);
            }

            // Indicate if this event *would* be a bounce based on current state
            if let Some(&last_us) = self.last_event_us.get(&key) {
                 if let Some(diff) = event_us.checked_sub(last_us) {
                     if diff < self.window_us {
                         eprint!(" (WOULD BE BOUNCE)");
                     }
                 }
            }
        }

        eprintln!(); // Newline after each event log
    }


    /// Prints collected statistics to the given writer (e.g., stderr). Only prints if verbose was enabled.
    pub fn print_stats(&self, writer: &mut impl Write) -> io::Result<()> {
        // Only print if stats were actually collected (verbose mode)
        if !self.is_verbose {
            // If verbose is off but log_events or bypass is on, print a minimal header
            if self.log_events || self.bypass {
                 writeln!(writer, "--- intercept-bounce status ---")?;
                 writeln!(writer, "Bypass mode: {}", if self.bypass { "Active" } else { "Inactive" })?;
                 writeln!(writer, "Event logging: {}", if self.log_events { "Active" } else { "Inactive" })?;
                 writeln!(writer, "-----------------------------")?;
            }
            return Ok(()); // No detailed stats if not verbose
        }

        // Detailed stats if verbose is on
        writeln!(writer, "--- intercept-bounce statistics ---")?;
        writeln!(writer, "Bypass mode: {}", if self.bypass { "Active" } else { "Inactive" })?;
        writeln!(writer, "Event logging: {}", if self.log_events { "Active" } else { "Inactive" })?;

        if !self.bypass { // Only print filtering stats if filtering was active
            writeln!(writer, "Window: {} µs", self.window_us)?;
            writeln!(writer, "Key events processed: {}", self.key_events_processed)?;
            writeln!(writer, "Key events dropped:   {}", self.key_events_dropped)?;
            let percentage = if self.key_events_processed > 0 {
                (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0 // Corrected f66 to f64
            } else {
                0.0
            };
            writeln!(writer, "Percentage dropped:   {:.2}%", percentage)?;

            // Print per-key stats if any keys were dropped
            if !self.per_key_dropped.is_empty() {
                writeln!(writer, "\nDropped events per key (code, value) [Name]: Count (Timing µs: min/avg/max)")?;
                // Sort by drop count descending for better readability
                let mut sorted_drops: Vec<_> = self.per_key_dropped.iter().collect();
                sorted_drops.sort_by(|a, b| b.1.cmp(a.1)); // Sort by count (b.1 vs a.1)

                for ((code, value), count) in sorted_drops {
                    let key_name = Self::get_key_name(*code);
                    let timing_stats = if let Some(timings) = self.per_key_timing.get(&(*code, *value)) {
                        if timings.is_empty() {
                            "N/A".to_string()
                        } else {
                            // Calculate min/max/avg safely
                            let min = timings.iter().min().unwrap_or(&0);
                            let max = timings.iter().max().unwrap_or(&0);
                            let sum: u64 = timings.iter().sum();
                            let avg = sum as f64 / timings.len() as f64;
                            format!("{}/{:.1}/{}", min, avg, max)
                        }
                    } else {
                        "N/A".to_string() // Should not happen if count > 0, but handle defensively
                    };

                    writeln!(writer, "  ({}, {}) [{}]: {} ({})", code, value, key_name, count, timing_stats)?;
                }
            }
        } else {
             // If bypass is active, filtering stats are not relevant
             writeln!(writer, "Filtering statistics are not available in bypass mode.")?;
        }


        writeln!(writer, "-----------------------------------")?;
        Ok(())
    }
}

// Helper functions from event.rs (copied here to avoid circular dependency or needing event.rs)
// In a real project, these would likely be in a shared module or event.rs would be added.

/// Returns true if the event type is EV_KEY.
#[inline]
pub fn is_key_event(event: &input_event) -> bool {
    i32::from(event.type_) == EV_KEY
}

/// Converts an input_event timeval to microseconds (u64).
#[inline]
pub fn event_microseconds(event: &input_event) -> u64 {
    // event.time is timeval { tv_sec: i64, tv_usec: i64 }
    // Convert tv_sec and tv_usec to u64 microseconds
    // Handle potential negative values from i64 by casting to u64,
    // assuming valid evdev timestamps are non-negative.
    (event.time.tv_sec.max(0) as u64) * 1_000_000 + (event.time.tv_usec.max(0) as u64)
}
