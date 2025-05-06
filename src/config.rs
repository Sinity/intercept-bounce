use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    debounce_time: Duration,
    near_miss_threshold: Duration,
    log_interval: Duration,
    pub log_all_events: bool,
    pub log_bounces: bool,
    pub stats_json: bool,
    pub verbose: bool,
    // Add log filter string
    pub log_filter: String,
    // OTLP endpoint
    pub otel_endpoint: Option<String>,
    // Ring buffer size for debugging
    pub ring_buffer_size: usize,
}

impl Config {
    /// Creates a new Config instance (primarily for testing/benchmarking).
    #[allow(clippy::too_many_arguments)] // Allow many args for test/bench helper
    pub fn new(
        debounce_time: Duration,
        near_miss_threshold: Duration,
        log_interval: Duration,
        log_all_events: bool,
        log_bounces: bool,
        stats_json: bool,
        verbose: bool,
        log_filter: String,
        otel_endpoint: Option<String>,
        ring_buffer_size: usize,
    ) -> Self {
        Self {
            debounce_time,
            near_miss_threshold,
            log_interval,
            log_all_events,
            log_bounces,
            stats_json,
            verbose,
            log_filter,
            otel_endpoint,
            ring_buffer_size,
        }
    }

    // Provide accessor methods that return Duration
    pub fn debounce_time(&self) -> Duration {
        self.debounce_time
    }
    pub fn near_miss_threshold(&self) -> Duration {
        self.near_miss_threshold
    }
    pub fn log_interval(&self) -> Duration {
        self.log_interval
    }

    // Provide accessor methods that return u64 microseconds for internal use
    pub fn debounce_us(&self) -> u64 {
        self.debounce_time
            .as_micros()
            .try_into()
            .unwrap_or(u64::MAX)
    }
    pub fn near_miss_threshold_us(&self) -> u64 {
        self.near_miss_threshold
            .as_micros()
            .try_into()
            .unwrap_or(u64::MAX)
    }
    pub fn log_interval_us(&self) -> u64 {
        self.log_interval.as_micros().try_into().unwrap_or(u64::MAX)
    }
}

impl From<&crate::cli::Args> for Config {
    fn from(a: &crate::cli::Args) -> Self {
        // Determine default log filter based on verbosity
        let default_log_filter = if a.verbose {
            "intercept_bounce=debug"
        } else {
            "intercept_bounce=info"
        };
        // Allow overriding with RUST_LOG environment variable
        let log_filter =
            std::env::var("RUST_LOG").unwrap_or_else(|_| default_log_filter.to_string()); // Keep to_string

        Self {
            debounce_time: a.debounce_time,
            near_miss_threshold: a.near_miss_threshold_time,
            log_interval: a.log_interval,
            log_all_events: a.log_all_events,
            log_bounces: a.log_bounces,
            stats_json: a.stats_json,
            verbose: a.verbose,
            log_filter,
            otel_endpoint: a.otel_endpoint.clone(),
            ring_buffer_size: a.ring_buffer_size,
        }
    }
}
