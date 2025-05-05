//! Utility functions shared across modules.

use std::time::Duration;

/// Formats a duration in microseconds into a human-readable string (µs, ms, or s).
#[inline]
pub fn format_us(us: u64) -> String {
    if us < 1000 {
        format!("{us} µs")
    } else if us < 1_000_000 {
        format!("{:.1} ms", us as f64 / 1000.0)
    } else {
        format!("{:.3} s", us as f64 / 1_000_000.0)
    }
}

/// Formats a `std::time::Duration` into a human-readable string using `humantime`.
#[inline]
pub fn format_duration(duration: Duration) -> String {
    humantime::format_duration(duration).to_string()
}
