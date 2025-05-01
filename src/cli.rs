use clap::Parser;

/// An Interception Tools filter to eliminate keyboard chatter (switch bounce).
/// Reads Linux input events from stdin, filters rapid duplicate key events,
/// and writes the filtered events to stdout. Statistics are printed to stderr on exit.
#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = "An Interception Tools filter to eliminate keyboard chatter (switch bounce).\n\
Reads Linux input events from stdin, filters rapid duplicate key events, and writes the filtered events to stdout.\n\
Statistics are printed to stderr on exit.\n\
\n\
EXAMPLES:\n\
  # Basic filtering (15ms window):\n\
  sudo sh -c 'intercept -g /dev/input/by-id/your-keyboard-event-device | intercept-bounce --debounce-time 15 | uinput -d /dev/input/by-id/your-keyboard-event-device'\n\
\n\
  # Filtering with bounce logging:\n\
  sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 20 --log-bounces | uinput -d ...'\n\
\n\
  # Debugging - log all events (no filtering):\n\
  sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 0 --log-all-events | uinput -d ...'\n\
\n\
  # Periodic stats dump:\n\
  sudo sh -c 'intercept -g ... | intercept-bounce --log-interval 60 | uinput -d ...'\n\
\n\
  # udevmon integration (YAML):\n\
  - JOB: \"intercept -g $DEVNODE | intercept-bounce | uinput -d $DEVNODE\"\n\
    DEVICE:\n\
      LINK: \"/dev/input/by-id/usb-Your_Keyboard_Name-event-kbd\"\n\
\n\
See README for more details and advanced usage."
)]
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
    pub list_devices: bool,

    /// Output statistics as JSON to stderr on exit and periodic dump.
    #[arg(long, action = clap::ArgAction::SetTrue)]
    pub stats_json: bool,
}

pub fn parse_args() -> Args {
    Args::parse()
}
