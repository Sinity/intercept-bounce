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
        let event_us = event::event_microseconds(event);
        let key_code = event.code;
        let key_value = event.value;

        if self.overall_first_event_us.is_none() {
            self.overall_first_event_us = Some(event_us);
        }
        self.overall_last_event_us = Some(event_us);

        let mut is_bounce = false;
        let mut diff_us_if_bounce = None;
        let mut last_passed_us_opt = None;

        if is_key_event(event) && key_value != 2 {
            let key_code_idx = key_code as usize;
            let key_value_idx = key_value as usize;

            if key_code_idx < 1024 && key_value_idx < 3 {
                let last_us_for_key_value = self.last_event_us[key_code_idx][key_value_idx];

                if last_us_for_key_value != u64::MAX {
                    last_passed_us_opt = Some(last_us_for_key_value);

                    // Check against Duration directly
                    if debounce_time > Duration::ZERO {
                        if let Some(diff) = event_us.checked_sub(last_us_for_key_value) {
                            if Duration::from_micros(diff) < debounce_time {
                                is_bounce = true;
                                diff_us_if_bounce = Some(diff);
                            }
                        } else {
                            is_bounce = false;
                        }
                    }
                }

                if !is_bounce {
                    self.last_event_us[key_code_idx][key_value_idx] = event_us;
                }
            } else {
                is_bounce = false;
            }
        } else {
            is_bounce = false;
        }

        (is_bounce, diff_us_if_bounce, last_passed_us_opt)
    }

    /// Returns the total duration based on the first and last event timestamps seen.
    /// Returns `None` if no events were processed.
    pub fn get_runtime_us(&self) -> Option<u64> {
        self.overall_last_event_us.and_then(|last| {
            self.overall_first_event_us.map(|first| last.saturating_sub(first))
        })
    }
}
