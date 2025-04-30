use input_linux_sys::input_event;
use std::collections::HashMap;

/// Holds the state for bounce filtering.
pub struct BounceFilter {
    window_us: u64,
    last_event_us: HashMap<u16, u64>, // Map key code to last event timestamp (µs)
}

impl BounceFilter {
    /// Creates a new BounceFilter.
    /// `window_ms`: The time window in milliseconds. Events within this window are filtered.
    pub fn new(window_ms: u64) -> Self {
        BounceFilter {
            window_us: window_ms * 1_000, // Convert ms to µs
            last_event_us: HashMap::new(),
        }
    }

    /// Determines if an event should be filtered (is a bounce).
    /// Updates the internal state if the event is not filtered.
    /// `event`: The key event to check.
    /// `event_us`: The timestamp of the event in microseconds.
    /// Returns `true` if the event is a bounce and should be dropped, `false` otherwise.
    pub fn is_bounce(&mut self, event: &input_event, event_us: u64) -> bool {
        match self.last_event_us.get(&event.code) {
            Some(&last_us) => {
                // Check if the time difference is within the window.
                // Use checked_sub to handle potential time jumps backwards gracefully.
                if event_us.checked_sub(last_us).map_or(false, |diff| diff < self.window_us) {
                    // Event is within the bounce window
                    true
                } else {
                    // Event is outside the bounce window, update timestamp
                    self.last_event_us.insert(event.code, event_us);
                    false
                }
            }
            None => {
                // First event for this key code, never a bounce. Record it.
                self.last_event_us.insert(event.code, event_us);
                false
            }
        }
    }
}
