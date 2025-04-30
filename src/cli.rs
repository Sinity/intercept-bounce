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
}

/// Parses command line arguments using clap.
pub fn parse_args() -> Args {
    Args::parse()
}
