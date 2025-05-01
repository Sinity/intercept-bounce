// This module defines the core BounceFilter state and logic.
// It has been refactored to be stateless regarding historical statistics,
// focusing only on the information needed for the immediate bounce decision.

pub mod keynames; // Keep keynames submodule accessible
pub mod stats; // Keep stats submodule accessible

use crate::event::{self, is_key_event}; // Use event module functions
use input_linux_sys::input_event;

/// Holds the minimal state required for bounce filtering decisions.
///
/// This struct only stores the timestamp of the last *passed* event
/// for each key code and value combination. It does not store historical
/// statistics; that responsibility is delegated to the `Logger` thread.
#[derive(Debug)] // Add Debug derive
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
            // Initialize all last event timestamps to MAX.
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
    /// * `debounce_time_us`: The debounce threshold in microseconds.
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
        debounce_time_us: u64,
    ) -> (bool, Option<u64>, Option<u64>) {
        // Calculate the event's timestamp in microseconds.
        let event_us = event::event_microseconds(event);
        let key_code = event.code;
        let key_value = event.value;

        // Track the timestamp of the very first and very last events seen.
        // This happens even for events that might be dropped.
        if self.overall_first_event_us.is_none() {
            self.overall_first_event_us = Some(event_us);
        }
        self.overall_last_event_us = Some(event_us);

        let mut is_bounce = false;
        let mut diff_us_if_bounce = None; // Difference if it *is* a bounce
        let mut last_passed_us_opt = None; // Timestamp of the previous passed event

        // --- Bounce Check Logic ---
        // Only apply debouncing logic to EV_KEY events, and specifically only to
        // press (1) and release (0) values. Key repeats (2) are never debounced.
        if is_key_event(event) && key_value != 2 {
            // Ensure key code and value are within the bounds of our state array.
            let key_code_idx = key_code as usize;
            let key_value_idx = key_value as usize;

            if key_code_idx < 1024 && key_value_idx < 3 {
                // Retrieve the timestamp of the last *passed* event for this specific key/value.
                let last_us_for_key_value = self.last_event_us[key_code_idx][key_value_idx];

                // Check if a previous event of this type has actually passed.
                if last_us_for_key_value != u64::MAX {
                    // Store the timestamp of the previous passed event. This is returned
                    // regardless of whether the current event is a bounce, as the logger
                    // needs it for near-miss calculations on passed events.
                    last_passed_us_opt = Some(last_us_for_key_value);

                    // Only perform the actual bounce check if debounce time is > 0.
                    if debounce_time_us > 0 {
                        // Calculate the time difference since the last passed event.
                        // Use checked_sub to handle potential timestamp wrap-around or out-of-order events.
                        if let Some(diff) = event_us.checked_sub(last_us_for_key_value) {
                            // If the difference is less than the threshold, it's a bounce.
                            if diff < debounce_time_us {
                                is_bounce = true;
                                diff_us_if_bounce = Some(diff); // Store the difference for stats
                            }
                            // Note: Near-miss calculation (diff >= debounce && diff < threshold)
                            // is now handled by the logger thread using last_passed_us_opt.
                        } else {
                            // Time appeared to go backwards (event_us < last_us_for_key_value).
                            // Treat this as not a bounce. last_passed_us_opt still holds the previous time.
                            is_bounce = false;
                        }
                    } // else: debounce_time_us is 0, so is_bounce remains false.
                } // else: This is the first event of this type seen, so last_passed_us_opt remains None.

                // --- Update State ---
                // IMPORTANT: Only update the `last_event_us` state if the event *passed* the filter.
                if !is_bounce {
                    self.last_event_us[key_code_idx][key_value_idx] = event_us;
                }
            } else {
                // Key code or value out of expected range. Treat as not a bounce.
                // Log this? For now, just pass it through without updating state.
                is_bounce = false;
            }
        } else {
            // Not an event type we debounce (e.g., EV_SYN, EV_KEY repeat).
            is_bounce = false;
        }

        // Return the bounce decision, the difference if it was a bounce,
        // and the timestamp of the previous passed event.
        (is_bounce, diff_us_if_bounce, last_passed_us_opt)
    }

    /// Returns the total duration based on the first and last event timestamps seen.
    /// Returns `None` if no events were processed.
    pub fn get_runtime_us(&self) -> Option<u64> {
        // Requires both first and last timestamps to be set.
        self.overall_last_event_us.and_then(|last| {
            self.overall_first_event_us.map(|first| last.saturating_sub(first))
        })
    }
}

// Note: Logging functions (log_event, log_simple_bounce) and stats printing
// (print_stats) have been removed from BounceFilter. This responsibility
// now lies with the Logger thread.
