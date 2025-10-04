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
    debounce_keys: Vec<u16>,
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
        debounce_keys: Vec<u16>,
        ignored_keys: Vec<u16>,
    ) -> Self {
        let mut debounce_keys = debounce_keys;
        debounce_keys.sort_unstable();
        debounce_keys.dedup();
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
            debounce_keys,
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

    pub fn debounce_keys(&self) -> &[u16] {
        &self.debounce_keys
    }

    pub fn should_debounce(&self, key_code: u16) -> bool {
        if !self.debounce_keys.is_empty() {
            return self.debounce_keys.binary_search(&key_code).is_ok();
        }

        self.ignored_keys.binary_search(&key_code).is_err()
    }

    pub fn is_key_ignored(&self, key_code: u16) -> bool {
        !self.should_debounce(key_code)
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
            a.debounce_keys.clone(),
            a.ignore_keys.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Config;
    use std::time::Duration;

    fn base_config() -> Config {
        Config::new(
            Duration::from_millis(25),
            Duration::from_millis(100),
            Duration::from_secs(15 * 60),
            false,
            false,
            false,
            false,
            "intercept_bounce=info".to_string(),
            None,
            0,
            Vec::new(),
            Vec::new(),
        )
    }

    #[test]
    fn ignores_configured_keys_when_no_debounce_allowlist() {
        let cfg = Config::new(
            Duration::from_millis(25),
            Duration::from_millis(100),
            Duration::from_secs(15 * 60),
            false,
            false,
            false,
            false,
            "intercept_bounce=info".to_string(),
            None,
            0,
            Vec::new(),
            vec![30],
        );

        assert!(cfg.is_key_ignored(30));
        assert!(!cfg.should_debounce(30));
        assert!(cfg.should_debounce(31));
    }

    #[test]
    fn debounce_keys_take_precedence_over_ignore_keys() {
        let cfg = Config::new(
            Duration::from_millis(25),
            Duration::from_millis(100),
            Duration::from_secs(15 * 60),
            false,
            false,
            false,
            false,
            "intercept_bounce=info".to_string(),
            None,
            0,
            vec![30, 40],
            vec![30],
        );

        assert!(
            cfg.should_debounce(30),
            "allowlisted keys must be debounced even if ignored"
        );
        assert!(cfg.should_debounce(40));
        assert!(!cfg.should_debounce(31));
        assert!(!cfg.should_debounce(0));
    }

    #[test]
    fn should_debounce_respects_sorted_dedup_lists() {
        let cfg = Config::new(
            Duration::from_millis(25),
            Duration::from_millis(100),
            Duration::from_secs(15 * 60),
            false,
            false,
            false,
            false,
            "intercept_bounce=info".to_string(),
            None,
            0,
            vec![40, 30, 30],
            vec![10, 10],
        );

        assert!(cfg.should_debounce(30));
        assert!(cfg.should_debounce(40));
        assert!(!cfg.should_debounce(10));
    }

    #[test]
    fn base_config_debounces_all_keys_by_default() {
        let cfg = base_config();
        assert!(cfg.should_debounce(0));
        assert!(cfg.should_debounce(u16::MAX));
    }
}
