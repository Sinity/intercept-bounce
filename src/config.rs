#[derive(Clone, Debug)]
pub struct Config {
    pub debounce_us:     u64,
    pub log_interval_us: u64,
    pub log_all_events:  bool,
    pub log_bounces:     bool,
    pub stats_json:      bool,
    pub verbose:         bool,
}
impl From<&crate::cli::Args> for Config {
    fn from(a: &crate::cli::Args) -> Self {
        Self {
            debounce_us:     a.debounce_time * 1_000,
            log_interval_us: a.log_interval * 1_000_000,
            log_all_events:  a.log_all_events,
            log_bounces:     a.log_bounces,
            stats_json:      a.stats_json,
            verbose:         a.verbose,
        }
    }
}
