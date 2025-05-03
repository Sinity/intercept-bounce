// This module defines the core BounceFilter state and logic.
// It has been refactored to be stateless regarding historical statistics,
// focusing only on the information needed for the immediate bounce decision.

pub mod keynames;
pub mod stats;

use crate::event::{self, is_key_event};
use input_linux_sys::input_event;
use std::time::Duration; // Import Duration

/// Holds the minimal state required for bounce filtering decisions.
///
/// This struct only stores the timestamp (in microseconds) of the last *passed* event
/// for each key code and value combination. It does not store historical
/// statistics; that responsibility is delegated to the `Logger` thread.
#[derive(Debug)]
pub struct BounceFilter {
    // Stores the timestamp (in microseconds) of the last event that *passed* the filter
    // for a given key code (index 0..1023) and key value (index 0=release, 1=press, 2=repeat).
    // Initialized with u64::MAX to indicate no event has passed yet.
    last_event_us: Box<[[u64; 3]; 1024]>,
    // Timestamp of the very first event processed, used for calculating total runtime.
    overall_first_event_us: Option<u64>,
    // Timestamp of the very last event processed, used for calculating total runtime.
    overall_last_event_us: Option<u64>,
}

impl BounceFilter {
    /// Creates a new, stateless `BounceFilter`.
    #[must_use]
    pub fn new() -> Self {
        BounceFilter {
            last_event_us: Box::new([[u64::MAX; 3]; 1024]),
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
    /// A tuple: `(is_bounce, diff_us_if_bounce, last_passed_us_before_this)`
    /// * `is_bounce`: `true` if the event should be dropped, `false` otherwise.
    /// * `diff_us_if_bounce`: If `is_bounce` is true, contains the time difference (µs)
    ///   between this event and the last passed event. Otherwise `None`.
    /// * `last_passed_us_before_this`: The timestamp (µs) of the previous event
    ///   of the same key code and value that passed the filter, or `None` if this
    ///   is the first passed event of its type. This is needed by the logger
    ///   thread to calculate near-miss statistics.
    pub fn check_event(
        &mut self,
        event: &input_event,
        debounce_time: Duration, // Use Duration
    ) -> (bool, Option<u64>, Option<u64>) {
        // Calculate timestamp and update overall tracking
        let event_us = event::event_microseconds(event);
        if self.overall_first_event_us.is_none() {
            self.overall_first_event_us = Some(event_us);
        }
        self.overall_last_event_us = Some(event_us);

        // Only apply debounce logic to EV_KEY press (1) and release (0) events
        if !is_key_event(event) || event.value == 2 {
            return (false, None, None); // Not a bounce, pass through
        }

        // Check if key code and value indices are within bounds
        let key_code_idx = event.code as usize;
        let key_value_idx = event.value as usize;
        if !(key_code_idx < 1024 && key_value_idx < 3) {
            // Should not happen with standard keyboards, but handle defensively
            return (false, None, None); // Pass through if out of bounds
        }

        // Get the timestamp of the last passed event for this specific key/value
        let last_passed_us = self.last_event_us[key_code_idx][key_value_idx];

        // If no previous event passed for this key/value, it cannot be a bounce
        if last_passed_us == u64::MAX {
            self.last_event_us[key_code_idx][key_value_idx] = event_us; // Record as passed
            return (false, None, None); // Not a bounce, no previous passed event
        }

        // Calculate time difference if possible (handles time going backwards)
        if let Some(diff_us) = event_us.checked_sub(last_passed_us) {
            // Check if the difference is less than the debounce time (and debounce > 0)
            if debounce_time > Duration::ZERO && Duration::from_micros(diff_us) < debounce_time {
                // It's a bounce! Return bounce info. Do NOT update last_event_us.
                return (true, Some(diff_us), Some(last_passed_us));
            }
        }

        // If we reach here, the event is NOT a bounce (either diff >= debounce_time,
        // debounce_time is zero, or time went backwards).
        self.last_event_us[key_code_idx][key_value_idx] = event_us; // Record as passed
        // Return non-bounce, providing the timestamp of the previously passed event
        (false, None, Some(last_passed_us))
    }

    /// Returns the total duration based on the first and last event timestamps seen.
    /// Returns `None` if no events were processed.
    pub fn get_runtime_us(&self) -> Option<u64> {
        self.overall_last_event_us.and_then(|last| {
            self.overall_first_event_us.map(|first| last.saturating_sub(first))
        })
    }
}
