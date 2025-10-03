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
    ignored_keys: Vec<u16>,
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
        ignored_keys: Vec<u16>,
    ) -> Self {
        let mut ignored_keys = ignored_keys;
        ignored_keys.sort_unstable();
        ignored_keys.dedup();
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
            ignored_keys,
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

    pub fn ignored_keys(&self) -> &[u16] {
        &self.ignored_keys
    }

    pub fn is_key_ignored(&self, key_code: u16) -> bool {
        self.ignored_keys.binary_search(&key_code).is_ok()
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

        Config::new(
            a.debounce_time,
            a.near_miss_threshold_time,
            a.log_interval,
            a.log_all_events,
            a.log_bounces,
            a.stats_json,
            a.verbose,
            log_filter,
            a.otel_endpoint.clone(),
            a.ring_buffer_size,
            a.ignore_keys.clone(),
        )
    }
}
