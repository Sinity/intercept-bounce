use clap::Parser;

/// An Interception Tools filter to eliminate keyboard chatter (switch bounce).
/// Reads Linux input events from stdin, filters rapid duplicate key events,
/// and writes the filtered events to stdout. Statistics are printed to stderr on exit.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Debounce time threshold (milliseconds). Duplicate key events (same keycode and value)
    /// occurring faster than this threshold are discarded. (Default: 10ms)
    #[arg(short = 't', long, default_value = "10", value_name = "MS")]
    pub debounce_time: u64,

    /// Periodically dump statistics to stderr every S seconds (default: 0 = disabled).
    #[arg(long, default_value = "0", value_name = "SECONDS")]
    pub log_interval: u64,

    /// Log details of *every* incoming event to stderr ([PASS] or [DROP]).
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub log_all_events: bool,

    /// Log details of *only dropped* (bounced) key events to stderr.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub log_bounces: bool,

    /// List available input devices and their capabilities (requires root).
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub list_devices: bool, // Add this flag
}

pub fn parse_args() -> Args {
    Args::parse()
}
