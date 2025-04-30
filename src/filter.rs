use input_linux_sys::input_event;
use phf; // Import phf crate
use std::collections::HashMap;
use std::io::{self, Write}; // Added io imports

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

/// Holds the state for bounce filtering, tracking the last event time for each key code and value (press/release/repeat state).
pub struct BounceFilter {
    window_us: u64,
    is_verbose: bool,
    log_interval: u64,
    last_event_us: HashMap<(u16, i32), u64>, // Map (key code, value) -> last event timestamp (µs) for bounce check
    last_any_event_us: HashMap<u16, u64>,    // Map key code -> last event timestamp (µs) for repeat logging
    // --- Statistics (only updated if verbose) ---
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
    pub fn new(window_ms: u64, is_verbose: bool, log_interval: u64) -> Self {
        BounceFilter {
            window_us: window_ms * 1_000,
            is_verbose,
            log_interval,
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

    /// Determines if an event should be filtered (is a bounce).
    /// Updates the internal state if the event is not filtered.
    /// Determines if an event should be filtered (is a bounce).
    /// Updates internal statistics *only if `verbose` was enabled at creation*.
    /// `event`: The key event to check. Must be a key event (EV_KEY).
    /// `event_us`: The timestamp of the event in microseconds.
    /// Returns `true` if the event is a bounce and should be dropped, `false` otherwise.
    pub fn is_bounce(&mut self, event: &input_event, event_us: u64) -> bool {
        let key_code = event.code;
        let key_value = event.value;
        let key = (key_code, key_value);

        // --- Extended Repeat Logging (only if verbose) ---
        if self.is_verbose && key_value == 2 { // Check if it's a repeat event
            if let Some(&last_us) = self.last_any_event_us.get(&key_code) {
                let repeat_check_window_us = self.window_us.max(100_000); // max(window, 100ms)
                if let Some(diff) = event_us.checked_sub(last_us) {
                    if diff < repeat_check_window_us {
                         // Log repeats within the extended window, even if not dropped
                         // Using eprint! to avoid implicit newline if needed elsewhere
                         eprint!(
                            "[VERBOSE] Repeat: Key {} ({}), Value: {}, Time since last: {} µs\n",
                            Self::get_key_name(key_code), key_code, key_value, diff
                        );
                    }
                }
            }
        }
        // --- End Extended Repeat Logging ---

        // Update last seen time for *any* event on this key code (for repeat logging)
        // Do this regardless of verbosity, as it's needed for the repeat check logic itself.
        self.last_any_event_us.insert(key_code, event_us);


        // --- Bounce Check Logic ---
        let mut bounce_diff_us: Option<u64> = None; // Store diff if it's a bounce
        let is_bounce = match self.last_event_us.get(&key) {
            Some(&last_us) => {
                // Check if the time difference is within the bounce window.
                // Use checked_sub to handle potential time jumps backwards gracefully (treat as not a bounce).
                event_us.checked_sub(last_us).map_or(false, |diff| {
                    if diff < self.window_us {
                        // Event is within the bounce window -> IS a bounce
                        bounce_diff_us = Some(diff); // Store the diff for stats
                        true
                    } else {
                        // Event is outside the bounce window -> NOT a bounce
                        false
                    }
                })
            }
            None => {
                // First event for this key code + value combination, never a bounce.
                false
            }
        };
        // --- End Bounce Check Logic ---


        // --- Statistics Update & Periodic Dump (only if verbose) ---
        if self.is_verbose {
            self.key_events_processed += 1;

            if is_bounce {
                self.key_events_dropped += 1;
                // Increment per-key drop count
                *self.per_key_dropped.entry(key).or_insert(0) += 1;
                // Record timing diff for dropped event if available
                if let Some(diff) = bounce_diff_us {
                     self.per_key_timing.entry(key).or_default().push(diff);
                }
            } else {
                // Event is not a bounce, update timestamp for this specific key+value state
                self.last_event_us.insert(key, event_us);
            }

            // Check for periodic dump
            if self.log_interval > 0 && self.key_events_processed % self.log_interval == 0 {
                 eprintln!("\n--- Periodic Stats Dump (Event {}) ---", self.key_events_processed);
                 // Ignore errors writing periodic stats
                 let _ = self.print_stats(&mut io::stderr());
                 eprintln!("---------------------------------------\n");
            }
        } else if !is_bounce {
             // Still need to update last_event_us even if not verbose
             self.last_event_us.insert(key, event_us);
        }
        // --- End Statistics Update ---

        is_bounce
    }

    /// Prints collected statistics to the given writer (e.g., stderr). Only prints if verbose was enabled.
    pub fn print_stats(&self, writer: &mut impl Write) -> io::Result<()> {
        // Only print if stats were actually collected
        if !self.is_verbose {
            return Ok(());
        }

        writeln!(writer, "--- intercept-bounce statistics ---")?;
        writeln!(writer, "Key events processed: {}", self.key_events_processed)?;
        writeln!(writer, "Key events dropped:   {}", self.key_events_dropped)?;
        let percentage = if self.key_events_processed > 0 {
            (self.key_events_dropped as f64 / self.key_events_processed as f64) * 100.0
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

        writeln!(writer, "-----------------------------------")?;
        Ok(())
    }
}
