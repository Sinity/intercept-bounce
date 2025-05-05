// This module defines the core BounceFilter state and logic.
// It has been refactored to be stateless regarding historical statistics,
// focusing only on the information needed for the immediate bounce decision.

pub mod keynames;
pub mod stats;

use crate::event::{self, is_key_event};
use crate::logger::EventInfo;
use input_linux_sys::{input_event, KEY_MAX};
use std::time::Duration;

// Constants for filter state size
/// Number of key codes to track (0 to KEY_MAX inclusive).
pub const FILTER_MAP_SIZE: usize = KEY_MAX as usize + 1;
/// Number of key states (0=release, 1=press, 2=repeat).
pub const NUM_KEY_STATES: usize = 3;

/// Holds the minimal state required for bounce filtering decisions.
///
/// This struct only stores the timestamp (in microseconds) of the last *passed* event
/// for each key code and value combination.
pub struct BounceFilter {
    // Stores the timestamp (in microseconds) of the last event that *passed* the filter
    // for a given key code (index 0..KEY_MAX) and key value (index 0=release, 1=press, 2=repeat).
    // Initialized with u64::MAX to indicate no event has passed yet.
    last_event_us: [[u64; NUM_KEY_STATES]; FILTER_MAP_SIZE],
    // Ring buffer to store the last N *passed* events for debugging purposes.
    #[cfg(feature = "debug_ring_buffer")]
    recent_passed_events: [Option<input_event>; 64],
    #[cfg(feature = "debug_ring_buffer")]
    recent_event_idx: usize,
    // Timestamp of the very first event processed, used for calculating total runtime.
    overall_first_event_us: Option<u64>,
    // Timestamp of the very last event processed, used for calculating total runtime.
    overall_last_event_us: Option<u64>,
}

impl Default for BounceFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl BounceFilter {
    /// Creates a new, stateless `BounceFilter`.
    #[must_use]
    pub fn new() -> Self {
        BounceFilter {
            last_event_us: [[u64::MAX; NUM_KEY_STATES]; FILTER_MAP_SIZE],
            #[cfg(feature = "debug_ring_buffer")]
            recent_passed_events: [(); 64].map(|_| None),
            #[cfg(feature = "debug_ring_buffer")]
            recent_event_idx: 0,
            overall_first_event_us: None,
            overall_last_event_us: None,
        }
    }

    /// Checks an incoming event against the debounce filter state.
    ///
    /// Determines if the event is a bounce based on the `debounce_time_us`
    /// and the timestamp of the last passed event of the same type.
    /// Updates the internal state (`last_event_us`) *only* if the event passes.
    /// Also tracks the overall first and last event timestamps.
    ///
    /// # Arguments
    /// * `event`: The input event to check.
    /// * `debounce_time`: The debounce threshold as a `Duration`.
    ///
    /// # Returns
    /// An `EventInfo` struct containing the result of the check and relevant timestamps.
    pub fn check_event(&mut self, event: &input_event, debounce_time: Duration) -> EventInfo {
        let event_us = event::event_microseconds(event);

        // Update overall timestamps
        if self.overall_first_event_us.is_none() {
            self.overall_first_event_us = Some(event_us);
        }
        self.overall_last_event_us = Some(event_us);

        // --- Early returns for non-debounced events ---
        // Pass non-key events or key repeats immediately
        if !is_key_event(event) || event.value == 2 {
            return EventInfo {
                event: *event,
                event_us,
                is_bounce: false,
                diff_us: None,
                last_passed_us: None, // No relevant last_passed_us for non-debounced events
            };
        }

        // Check bounds for key code/value indices
        let key_code_idx = event.code as usize;
        let key_value_idx = event.value as usize;
        if !(key_code_idx < FILTER_MAP_SIZE && key_value_idx < NUM_KEY_STATES) {
            // Out of bounds - treat as passed, no relevant history
            return EventInfo {
                event: *event,
                event_us,
                is_bounce: false,
                diff_us: None,
                last_passed_us: None,
            };
        }

        // --- Debounce logic ---
        let last_passed_us = self.last_event_us[key_code_idx][key_value_idx];

        // If no previous event passed for this key/value, it cannot be a bounce. Record and pass.
        if last_passed_us == u64::MAX {
            self.last_event_us[key_code_idx][key_value_idx] = event_us;
            // Record passed event in ring buffer
            #[cfg(feature = "debug_ring_buffer")]
            {
                self.recent_passed_events[self.recent_event_idx] = Some(*event);
                self.recent_event_idx = (self.recent_event_idx + 1) % 64;
            }
            return EventInfo {
                event: *event, // Copy event
                event_us,
                is_bounce: false,
                diff_us: None,
                last_passed_us: None, // No previous passed event for this key/value
            };
        }

        // Calculate time difference if possible (handles time going backwards)
        let diff_us_opt = event_us.checked_sub(last_passed_us);

        if let Some(diff_us) = diff_us_opt {
            // Check if the difference is within the debounce window.
            if debounce_time > Duration::ZERO && Duration::from_micros(diff_us) < debounce_time {
                // It's a bounce! Return bounce info. Do NOT update last_event_us or ring buffer.
                return EventInfo {
                    event: *event,
                    event_us,
                    is_bounce: true,
                    diff_us: Some(diff_us),
                    last_passed_us: Some(last_passed_us),
                };
            }
        }
        // If time went backwards (checked_sub returned None), or diff_us >= debounce_time, it's not a bounce.

        // --- Event Passed ---
        // If we reach here, the event is NOT a bounce. Record as passed.
        self.last_event_us[key_code_idx][key_value_idx] = event_us;
        // Record passed event in ring buffer
        #[cfg(feature = "debug_ring_buffer")]
        {
            self.recent_passed_events[self.recent_event_idx] = Some(*event);
            self.recent_event_idx = (self.recent_event_idx + 1) % 64;
        }

        // Return non-bounce info, providing the timestamp of the previously passed event.
        EventInfo {
            event: *event,
            event_us,
            is_bounce: false,
            diff_us: None, // Not a bounce, so no bounce diff_us
            last_passed_us: Some(last_passed_us),
        }
    }

    /// Returns the total duration based on the first and last event timestamps seen.
    /// Returns `None` if no events were processed.
    pub fn get_runtime_us(&self) -> Option<u64> {
        self.overall_last_event_us.and_then(|last| {
            self.overall_first_event_us
                .map(|first| last.saturating_sub(first))
        })
    }
}
