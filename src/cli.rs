use clap::Parser;

/// Filter tool for Interception Tools to discard rapid duplicate key events (bounces).
/// Reads input_event structs from stdin and writes filtered events to stdout.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Time window (milliseconds). Duplicate key events (same keycode and value) occurring
    /// faster than this window are discarded. Higher value = more filtering.
    #[arg(short, long, default_value = "10")]
    pub window: u64,

    /// Collect and print statistics (including detailed bounce timings) on exit and periodically.
    #[arg(short, long, action = clap::ArgAction::SetTrue)]
    pub stats: bool, // Renamed from verbose

    /// Dump statistics to stderr every N key events processed (default: 0 = disabled). Requires --stats.
    #[arg(long, default_value = "0", value_name = "N")]
    pub log_interval: u64,

    // Removed bypass flag (use --window 0 instead)

    /// Log details of *every* incoming event to stderr (prefixed with [PASS] or [DROP]).
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub log_all_events: bool, // Renamed from log_events

    /// Log details of *only dropped* (bounced) key events to stderr.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub log_bounces: bool,
}

/// Parses command line arguments using clap.
pub fn parse_args() -> Args {
    Args::parse()
}
