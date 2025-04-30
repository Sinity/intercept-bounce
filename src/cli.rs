use clap::Parser;

/// Bounce-filter for Interception Tools
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Window (ms) within which repeat edges are discarded
    #[arg(short, long, default_value = "5")]
    pub window: u64,
}

pub fn parse_args() -> Args {
    Args::parse()
}
