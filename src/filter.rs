use input_linux_sys::input_event;
use std::collections::HashMap;

/// Holds the state for bounce filtering, tracking the last event time for each key code and value (press/release/repeat state).
pub struct BounceFilter {
    window_us: u64,
    last_event_us: HashMap<(u16, i32), u64>, // Map (key code, value) to last event timestamp (µs)
}

impl BounceFilter {
    /// Creates a new BounceFilter.
    /// `window_ms`: The time window in milliseconds. Events within this window are filtered.
    pub fn new(window_ms: u64) -> Self {
        BounceFilter {
            window_us: window_ms * 1_000, // Convert ms to µs
            // Use a reasonable initial capacity
            last_event_us: HashMap::with_capacity(64),
        }
    }

    /// Determines if an event should be filtered (is a bounce).
    /// Updates the internal state if the event is not filtered.
    /// `event`: The key event to check. Must be a key event (EV_KEY).
    /// `event_us`: The timestamp of the event in microseconds.
    /// Returns `true` if the event is a bounce and should be dropped, `false` otherwise.
    pub fn is_bounce(&mut self, event: &input_event, event_us: u64) -> bool {
        let key = (event.code, event.value);
        match self.last_event_us.get(&key) {
            Some(&last_us) => {
                // Check if the time difference is within the window.
                // Use checked_sub to handle potential time jumps backwards gracefully (treat as not a bounce).
                if event_us.checked_sub(last_us).map_or(false, |diff| diff < self.window_us) {
                    // Event is within the bounce window
                    // Event is within the bounce window -> IS a bounce
                    true
                } else {
                    // Event is outside the bounce window -> NOT a bounce
                    self.last_event_us.insert(key, event_us); // Update timestamp for this key+value
                    false
                }
            }
            None => {
                // First event for this key code + value combination, never a bounce. Record it.
                self.last_event_us.insert(key, event_us);
                false
            }
        }
    }
}
