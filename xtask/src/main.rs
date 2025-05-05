use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};
use clap_complete_nushell::Nushell;
use clap_mangen::Man;
use intercept_bounce::cli::Args; // Import Args from the library

use std::io::Write;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct XtaskArgs {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Generate man page and shell completions.
    GenerateDocs,
    /// Run cargo check.
    Check,
    /// Run cargo test.
    Test,
    /// Run cargo clippy.
    Clippy,
    /// Run cargo fmt --check.
    FmtCheck,
}

fn main() -> Result<()> {
    let args = XtaskArgs::parse();

    match args.command {
        Commands::GenerateDocs => generate_docs().context("Failed to generate docs"),
        Commands::Check => run_cargo("check", &[]).context("cargo check failed"),
        Commands::Test => run_cargo("test", &[]).context("cargo test failed"),
        Commands::Clippy => {
            run_cargo("clippy", &["--", "-D", "warnings"]).context("cargo clippy failed")
        }
        Commands::FmtCheck => run_cargo("fmt", &["--", "--check"]).context("cargo fmt failed"),
    }
}

fn run_cargo(command: &str, args: &[&str]) -> Result<()> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.arg(command);
    cmd.args(args);
    // Run in the workspace root
    cmd.current_dir(project_root());

    let status = cmd
        .status()
        .context(format!("Failed to execute cargo {}", command))?;

    if !status.success() {
        anyhow::bail!("cargo {} command failed", command);
    }
    Ok(())
}

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

fn generate_docs() -> Result<()> {
    let root_dir = project_root();
    let docs_dir = root_dir.join("docs");
    let man_dir = docs_dir.join("man");
    let completions_dir = docs_dir.join("completions");

    fs::create_dir_all(&man_dir).context("Failed to create man directory")?;
    fs::create_dir_all(&completions_dir).context("Failed to create completions directory")?;

    let cmd = Args::command();
    let bin_name = cmd.get_name().to_string(); // Get bin name from clap

    // --- Generate Man Page ---
    let man_path = man_dir.join(format!("{}.1", bin_name));
    println!("Generating man page: {:?}", man_path);
    generate_man_page(&cmd, &man_path)?;

    // --- Generate Shell Completions ---
    generate_completions(&cmd, &completions_dir)?;

    println!(
        "Successfully generated man page and completions in: {}",
        docs_dir.display()
    );
    Ok(())
}

// --- Man Page Content Constants ---
// Note: Using roff formatting. \fB...\fR = bold, \fI...\fR = italic, \- = hyphen, \(bu = bullet

const MAN_DESCRIPTION: &str = r#"
\fB{bin_name}\fR is an Interception Tools filter designed to eliminate keyboard chatter (also known as switch bounce).
It reads Linux \fBinput_event\fR(5) structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output.
Statistics are printed to standard error on exit or periodically.

Keyboard chatter occurs when a single physical key press or release generates multiple electrical signals due to the mechanical contacts bouncing. This can result in unintended duplicate characters or actions. \fB{bin_name}\fR addresses this by ignoring subsequent identical key events (same key code and same state \- press or release) that occur within the specified \fB\-\-debounce\-time\fR.
.PP
The filter maintains state independently for each key code (0-1023) and for each state (press=1, release=0). Key repeat events (value=2) are never filtered. Non-key events (e.g., mouse movements, synchronization events) are passed through unmodified.
.PP
It is designed for the Linux environment using the \fBevdev\fR input system and integrates seamlessly with the Interception Tools ecosystem, typically placed in a pipeline between \fBintercept\fR(1) (to capture events) and \fBuinput\fR(1) (to create a filtered virtual device).
.PP
Performance is critical for input filtering. \fB{bin_name}\fR uses a mutex to protect the core filter state and a separate thread for logging and statistics to minimize latency impact on the main event processing path. See the PERFORMANCE section for more details.
"#;

const MAN_DEBOUNCING: &str = r#"
.B Mechanism
.PP
\fB{bin_name}\fR works by remembering the timestamp of the last \fIpassed\fR event for each unique combination of key code (e.g., KEY_A) and key state (press=1, release=0).
When a new key event arrives, it checks if it's identical to the last passed event for that key/state combination.
If it is identical and the time difference since the last passed event is \fIless than\fR the configured \fB\-\-debounce\-time\fR, the new event is considered a bounce and is dropped (not written to standard output).
If the time difference is greater than or equal to the debounce time, or if the event is different (different key code or different state), the event is passed through and its timestamp is recorded as the new "last passed" time for that specific key/state.
.PP
Key repeat events (value=2) are always passed through without debouncing, as they represent intentional key holds.
.PP
.B Choosing \-\-debounce\-time
.IP \(bu 4
Start with the default (\fB25ms\fR) or a common value like \fB15ms\fR.
.IP \(bu 4
If you experience missed keystrokes, the debounce time might be too high. Try lowering it (e.g., to 10ms or 5ms).
.IP \(bu 4
If you still experience duplicate characters (chatter), the debounce time might be too low.
.IP \(bu 4
To diagnose chatter on specific keys, run with \fB\-\-log\-bounces\fR and a low debounce time (e.g., \fB\-\-debounce\-time 5ms\fR). Observe the logs to see which keys are frequently dropped and the typical time difference reported for the bounces. Increase the debounce time to be slightly higher than the observed bounce times for problematic keys.
.IP \(bu 4
Typical values range from 5ms to 30ms. Mechanical switches might require slightly higher values than membrane switches.
.IP \(bu 4
Setting \fB\-\-debounce\-time 0ms\fR effectively disables filtering, passing all events through. This is useful with \fB\-\-log\-all\-events\fR to observe raw event timings.
"#;

const MAN_NEAR_MISS: &str = r#"
.B Purpose
.PP
The near-miss feature is a diagnostic tool to identify key presses or releases that occur just slightly \fIafter\fR the debounce window closes. It helps understand key timing consistency and identify keys that might be close to chattering.
.PP
.B Mechanism
.PP
When a key event \fIpasses\fR the debounce filter (i.e., it's not a bounce), \fB{bin_name}\fR calculates the time difference since the \fIprevious passed event\fR for the same key code and state. If this difference is less than or equal to the \fB\-\-near\-miss\-threshold\-time\fR (and greater than or equal to the \fB\-\-debounce\-time\fR, which is implicit for passed events), it's recorded as a near-miss.
.PP
.B Interpretation
.IP \(bu 4
High near-miss counts for a specific key suggest its timing is inconsistent or very close to the debounce threshold.
.IP \(bu 4
This might indicate a switch that is starting to fail or that the \fB\-\-debounce\-time\fR might need a slight increase for that particular keyboard.
.IP \(bu 4
Near-miss statistics are primarily useful for fine-tuning the debounce time or identifying potentially problematic hardware switches.
.PP
.B Configuration
.IP \(bu 4
The \fB\-\-near\-miss\-threshold\-time\fR should generally be set significantly higher than the \fB\-\-debounce\-time\fR (e.g., 100ms threshold vs 20ms debounce) to capture relevant events without being overly noisy.
.IP \(bu 4
Setting the threshold to the same value as the debounce time (or 0ms) effectively disables near-miss tracking.
"#;

const MAN_EXAMPLES: &str = r#"
.PP
.B Basic Filtering (15ms window):
.IP
.nf
sudo sh \-c 'intercept \-g /dev/input/by\-id/your\-kbd | {bin_name} \-\-debounce\-time 15ms | uinput \-d /dev/input/by\-id/your\-kbd'
.fi
.PP
Intercept events from your keyboard (replace placeholder path), filter them with a 15ms debounce window, and create a new virtual keyboard device with the filtered output. Applications should use the new virtual device created by \fBuinput\fR.
.PP
.B Filtering with Bounce Logging:
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-debounce\-time 20ms \-\-log\-bounces | uinput \-d ...'
.fi
.PP
Filter with a 20ms threshold and log only the key events that are dropped (considered bounces) to standard error. Useful for identifying which keys are chattering without logging every event.
.PP
.B Diagnosing Chatter Timing:
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-debounce\-time 5ms \-\-log\-bounces | uinput \-d ...'
.fi
.PP
Run with a low debounce time (5ms) and log bounces. Observe the reported time differences for dropped events to determine an appropriate debounce time for your keyboard (set it slightly higher than the observed bounce times).
.PP
.B Periodic Stats Dump (every 5 minutes):
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-log\-interval 5m | uinput \-d ...'
.fi
.PP
Run with default filtering and print detailed statistics to standard error every 5 minutes, in addition to the final report on exit.
.PP
.B JSON Statistics Output:
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-stats\-json | uinput \-d ...' > /dev/null
.fi
.PP
Output statistics in JSON format to standard error. Standard output (the filtered events) is redirected to /dev/null in this example, useful if only collecting stats.
.PP
.B Finding Your Keyboard Device:
.IP Use \fBintercept \-L\fR or look in \fI/dev/input/by-id/\fR for device names ending in \fI-event-kbd\fR.
.fi
"#;

const MAN_INTEGRATION: &str = r#"
\fB{bin_name}\fR is designed to work with Interception Tools. It can be used in pipelines or within a \fBudevmon\fR(1) configuration file (\fIudevmon.yaml\fR).
.PP
.B Pipeline Usage
.PP
The standard usage involves a pipeline: \fBintercept\fR -> \fB{bin_name}\fR -> \fBuinput\fR.
.IP \(bu 4
\fBintercept \-g <device>\fR: Captures raw input events from the specified hardware device.
.IP \(bu 4
\fB{bin_name} [OPTIONS]\fR: Reads events from stdin, filters them, and writes filtered events to stdout.
.IP \(bu 4
\fBuinput \-d <device>\fR: Reads filtered events from stdin and creates a new virtual input device mirroring the original device's capabilities but emitting only the filtered events.
.PP
.B Virtual Device
.PP
It is crucial to understand that \fBuinput\fR creates a \fInew\fR virtual input device (e.g., /dev/input/eventX). Your applications and desktop environment (Xorg/Wayland) must be configured to use \fIthis new virtual device\fR instead of the original physical keyboard device. The original device will still emit raw, unfiltered events. How to configure the desktop environment varies; sometimes it picks up the new device automatically, other times specific Xorg configuration (e.g., `InputDevice` section with `Option "GrabDevice" "True"`) or Wayland compositor settings might be needed. Check your desktop environment's documentation. You can often identify the virtual device by looking for "Uinput" in its name via tools like \fBlibinput list-devices\fR or checking devices created after the pipeline starts.
.PP
.B udevmon Integration
.PP
Using \fBudevmon\fR is often the most robust way to manage the pipeline, automatically starting and stopping it when the keyboard is connected or disconnected.
.IP
.nf
# Example /etc/udevmon.yaml entry
\- JOB: "intercept \-g $DEVNODE | {bin_name} \-\-debounce\-time 15ms | uinput \-d $DEVNODE"
  DEVICE:
    LINK: "/dev/input/by\-id/usb\-Your_Keyboard_Name\-event\-kbd"
.fi
.PP
Replace the \fILINK\fR value with the appropriate path for your keyboard found in \fI/dev/input/by-id/\fR. The \fI$DEVNODE\fR variable is automatically substituted by \fBudevmon\fR with the actual device path (e.g., /dev/input/event5). Refer to the Interception Tools documentation for more details on configuring \fBudevmon\fR.
.PP
.B Wayland/Xorg Considerations
.PP
Interception Tools, especially global input capture, generally work most reliably under Xorg. Wayland compositors often restrict global input grabbing for security reasons. Using \fB{bin_name}\fR under Wayland might require specific compositor support (e.g., `libei` integration in the compositor) or might only work for applications that directly use the virtual device created by `uinput`. Check your Wayland compositor's documentation regarding input device management and potential compatibility with virtual input devices.
"#;

const MAN_STATISTICS: &str = r#"
\fB{bin_name}\fR collects and reports detailed statistics about the events it processes. These statistics provide insights into keyboard chatter patterns and filter effectiveness.
.PP
.B Metrics Reported (Human-Readable):
.IP "\fBOverall Statistics\fR" 4
Includes total events processed, total key events processed, key events passed, key events dropped, and the overall drop percentage for key events.
.IP "\fBPer-Key Statistics\fR" 4
For each key code that had events processed:
.RS 4
.IP \(bu 4
Total processed events for that key.
.IP \(bu 4
Total dropped events (bounces) for that key.
.IP \(bu 4
Drop percentage for that key.
.IP \(bu 4
Detailed stats for \fBPress\fR (value=1) and \fBRelease\fR (value=0) states:
.RS 4
.IP \(bu 4
\fBProcessed\fR: Number of press/release events seen for this key.
.IP \(bu 4
\fBDropped\fR: Number of press/release events dropped (bounced).
.IP \(bu 4
\fBBounce Time (Min/Avg/Max)\fR: The time difference (µs) between a dropped event and the previous \fIpassed\fR event of the same key and state. Provides insight into how quickly bounces occur.
.RE
.RE
.IP "\fBNear-Miss Statistics\fR" 4
For each key code/state combination with near-misses:
.RS 4
.IP \(bu 4
\fBCount\fR: Number of passed events that qualified as near-misses.
.IP \(bu 4
\fBNear-Miss Time (Min/Avg/Max)\fR: The time difference (µs) between a passed near-miss event and the previous \fIpassed\fR event of the same key and state.
.RE
.PP
.B Interpretation:
.IP \(bu 4
High overall drop percentage indicates significant chatter.
.IP \(bu 4
High per-key drop counts pinpoint specific problematic keys.
.IP \(bu 4
Bounce timings help determine an appropriate \fB\-\-debounce\-time\fR (set it slightly above the average or max bounce time).
.IP \(bu 4
Near-miss timings indicate events just outside the debounce window, suggesting potential need for adjustment or failing hardware.
.PP
.B JSON Output (\-\-stats\-json):
.IP
Provides a machine-readable format containing all the same information. Key fields include:
.RS 4
.IP "\fBreport_type\fR": "Cumulative" or "Periodic".
.IP "\fBruntime_duration_us\fR": Total runtime in microseconds (cumulative only).
.IP "\fBdebounce_time_us\fR", "\fBnear_miss_threshold_us\fR", "\fBlog_interval_us\fR": Configuration values used.
.IP "\fBkey_events_processed\fR", "\fBkey_events_passed\fR", "\fBkey_events_dropped\fR": Overall counts.
.IP "\fBper_key_stats\fR": Array of objects, each containing `key_code`, `key_name`, `total_processed`, `total_dropped`, `drop_percentage`, and detailed `press_`/`release_`/`repeat_` fields (e.g., `press_dropped_count`, `press_drop_timings_us`).
.IP "\fBper_key_passed_near_miss_timing\fR": Array of objects, each containing `key_code`, `key_value`, `key_name`, `value_name`, `count`, `timings_us`, `min_us`, `avg_us`, `max_us`.
.RE
"#;

const MAN_LOGGING: &str = r#"
\fB{bin_name}\fR provides several logging options for debugging and monitoring, written to standard error:
.PP
.B Log Flags:
.IP "\fB\-\-log\-all\-events\fR" 4
Log details of every incoming event ([PASS] or [DROP]). Useful for seeing the full event stream and filter decisions. Note: `EV_SYN` and `EV_MSC` events are skipped for cleaner output by default in this mode.
.IP "\fB\-\-log\-bounces\fR" 4
Log details of *only dropped* (bounced) key events. Less verbose than `--log-all-events`, focusing on problematic events. Ignored if `--log-all-events` is active.
.IP "\fB\-\-verbose\fR" 4
Enable verbose logging (DEBUG level). Includes internal state information, thread startup/shutdown messages, mutex acquisition attempts, etc. Also sets the default log filter level if `RUST_LOG` is not set.
.PP
.B Log Format:
.IP
Log lines typically include:
.RS 4
.IP \(bu 4
Timestamp (ISO 8601 format).
.IP \(bu 4
Log level (e.g., INFO, DEBUG, TRACE).
.IP \(bu 4
Status ([PASS] or [DROP]).
.IP \(bu 4
Relative time since first event (e.g., `+123.4ms`).
.IP \(bu 4
Event details (Type, Code, Value).
.IP \(bu 4
Key name (e.g., `KEY_A`).
.IP \(bu 4
Bounce/Near-Miss timing info (e.g., `bounce_us=5123`, `near_miss_us=26789`).
.RE
.PP
.B Environment Variable: RUST_LOG
.IP
Provides fine-grained control over logging. Overrides defaults set by `--verbose`. Syntax examples:
.RS 4
.IP "\fBRUST_LOG=info\fR" 4
Show INFO level messages and above (default without `--verbose`).
.IP "\fBRUST_LOG=debug\fR" 4
Show DEBUG level messages and above (equivalent to `--verbose`).
.IP "\fBRUST_LOG=trace\fR" 4
Show all messages, including TRACE level (very verbose).
.IP "\fBRUST_LOG=intercept_bounce=debug\fR" 4
Show DEBUG messages only from the `intercept_bounce` crate.
.IP "\fBRUST_LOG=info,intercept_bounce::filter=trace\fR" 4
Set INFO level globally, but TRACE level for the `filter` module.
.RE
.IP
See the \fBtracing_subscriber\fR documentation for the full filter syntax.
.PP
.B Performance Note:
Enabling \fB\-\-log\-all\-events\fR or high verbosity levels (trace) can significantly impact performance due to the volume of log messages generated and potential contention writing to stderr. Use primarily for debugging specific issues.
"#;

const MAN_SIGNALS: &str = r#"
\fB{bin_name}\fR handles the following signals gracefully to ensure clean shutdown and reporting of final statistics:
.IP \(bu 4
SIGINT (Interrupt, typically Ctrl+C)
.IP \(bu 4
SIGTERM (Termination signal)
.IP \(bu 4
SIGQUIT (Quit signal)
.PP
When any of these signals are received, the program will:
.IP 1. 4
Stop reading new input events.
.IP 2. 4
Signal the logger thread to stop processing queued messages.
.IP 3. 4
Wait for the logger thread to finish and return the final cumulative statistics.
.IP 4. 4
Print the final cumulative statistics to standard error (unless `--stats-json` is used, in which case JSON is printed).
.IP 5. 4
Exit cleanly (typically with status 0).
.PP
This ensures that valuable statistics are not lost even if the filter is terminated externally.
"#;

const MAN_EXIT_STATUS: &str = r#"
.IP 0 4
Success. The program completed normally or was terminated cleanly by a handled signal (SIGINT, SIGTERM, SIGQUIT). Final statistics were printed.
.IP 1 4
Runtime Error. An unexpected error occurred during execution, such as:
.RS 4
.IP \(bu 4
Error reading from standard input or writing to standard output.
.IP \(bu 4
Error creating or communicating with the logger thread.
.IP \(bu 4
Internal logic errors (panics).
.IP \(bu 4
Errors initializing OpenTelemetry (if configured).
.RE
.IP 2 4
Device Listing Error. An error occurred when using the \fB\-\-list\-devices\fR option, likely due to insufficient permissions to access \fI/dev/input/event*\fR devices.
"#;

const MAN_ENVIRONMENT: &str = r#"
.TP
.B RUST_LOG
Controls the logging verbosity and filtering, overriding defaults set by \fB\-\-verbose\fR. See the LOGGING section for details and examples. Uses the \fBtracing_subscriber::EnvFilter\fR format.
.TP
.B RUST_BACKTRACE
Set to \fB1\fR or \fBfull\fR to enable backtraces on panic, which can be helpful for debugging crashes.
"#;

const MAN_PERFORMANCE: &str = r#"
\fB{bin_name}\fR is designed for low-latency input filtering.
.PP
.B Architecture:
.IP \(bu 4
\fBMain Thread\fR: Reads events from stdin, acquires a mutex lock on the filter state, performs the debounce check (array lookup and comparison), updates state if necessary, releases the lock, sends event info to the logger thread via a channel, and writes passed events to stdout.
.IP \(bu 4
\fBLogger Thread\fR: Receives event info from the channel, updates statistics (lock-free atomics for simple counts, Vecs for timings), handles periodic stats dumps, and performs logging to stderr.
.PP
.B Latency Considerations:
.IP \(bu 4
The primary latency contributor on the main path is the mutex lock for the filter state (`BounceFilter::last_event_us`). This lock is held briefly during the check.
.IP \(bu 4
Sending data to the logger thread uses a bounded channel (`crossbeam-channel`). If the logger thread falls behind (e.g., due to slow disk I/O for logging or high CPU load), the channel might fill up. To prevent blocking the main input path, the main thread uses `try_send`. If the channel is full, a warning is logged ("Logger channel full, dropping log messages"), and the event info is \fIdropped\fR (not sent to the logger). This prioritizes low input latency over potentially losing some log messages or stats updates under heavy load.
.IP \(bu 4
Heavy logging (especially `--log-all-events` or TRACE level) significantly increases the work done by the logger thread and the likelihood of the channel filling up. It also increases overall CPU usage.
.PP
.B Benchmarking:
.IP
Use \fBcargo bench\fR to run microbenchmarks measuring the performance of the core filter logic (`BounceFilter::check_event`) and the inter-thread channel communication.
"#;

const MAN_BUGS: &str = r#"
Please report bugs, issues, or feature requests via the GitHub issue tracker:
https://github.com/sinity/intercept-bounce/issues
"#;

const MAN_SEE_ALSO: &str = r#"
\fBintercept\fR(1), \fBuinput\fR(1), \fBudevmon\fR(1), \fBinput_event\fR(5), \fBlibinput\fR(1), \fBeudev\fR(7), \fBsystemd-udevd.service\fR(8)
.PP
Interception Tools Project: https://gitlab.com/interception/linux/tools
.PP
Project Repository & README: https://github.com/sinity/intercept-bounce
"#;

const MAN_TROUBLESHOOTING: &str = r#"
.TP
.B Permission Denied (reading /dev/input/event* or writing /dev/uinput):
Running \fBintercept\fR and \fBuinput\fR typically requires root privileges or specific user group memberships. Ensure the user running the pipeline has read access to the target input device (\fI/dev/input/event*\fR) and write access to \fI/dev/uinput\fR. Adding the user to the 'input' group often grants read access, but write access to `/dev/uinput` might require specific udev rules (see Interception Tools documentation). Running the entire pipeline via `sudo sh -c '...'` is a common workaround.
.TP
.B Incorrect Device Path:
Double-check the device path used with \fBintercept \-g\fR or in \fIudevmon.yaml\fR. Use stable paths from \fI/dev/input/by-id/\fR or \fI/dev/input/by-path/\fR instead of potentially unstable \fI/dev/input/eventX\fR paths. Use \fBintercept \-L\fR to list devices.
.TP
.B Filter Not Working / No Output from uinput:
.RS 4
.IP \(bu 4
Verify the pipeline order: `intercept | {bin_name} | uinput`.
.IP \(bu 4
Check permissions (see above).
.IP \(bu 4
Ensure the correct device path is used for both `intercept -g` and `uinput -d`.
.IP \(bu 4
Check if `udevmon` (if used) is running and loaded the correct configuration (`sudo systemctl status udevmon`). Check `udevmon` logs (`journalctl -u udevmon`).
.IP \(bu 4
Run `{bin_name}` with `--verbose` or `--log-all-events` to see if events are being processed and passed/dropped as expected. Check for errors logged to stderr.
.IP \(bu 4
Ensure your desktop environment is configured to use the virtual device created by `uinput` (see INTEGRATION section).
.RE
.TP
.B Too Much Filtering (Missed Keystrokes):
The `--debounce-time` might be too high. Try lowering it. Use logging to see if legitimate presses are being dropped.
.TP
.B Too Little Filtering (Chatter Still Occurs):
The `--debounce-time` might be too low. Use `--log-bounces` with a low debounce time to measure chatter timing, then set the debounce time slightly higher.
.TP
.B Mixed Output in Terminal:
If running interactively with logging enabled, log messages (stderr) might mix with terminal echo or shell output. Redirect stderr (\fI2> logfile.txt\fR) or use \fBudevmon\fR for background operation.
.TP
.B "Logger channel full, dropping log messages" Warning:
This means the logger thread cannot keep up with the rate of events from the main thread, likely due to heavy logging or high system load. Log messages and stats updates might be lost, but input filtering latency is prioritized. Reduce logging verbosity (`--log-all-events`, `RUST_LOG` level) if this occurs frequently.
.TP
.B Errors Reading/Writing Events:
Check device permissions, physical keyboard connection, and system logs (`dmesg`) for hardware errors related to the input device.
"#;

const MAN_THEORY_OF_OPERATION: &str = r#"
\fB{bin_name}\fR employs a multi-threaded architecture to separate the low-latency input filtering path from potentially slower logging and statistics processing.
.PP
.B Main Thread:
.IP 1. 4
Reads raw `input_event` structs from standard input in a loop.
.IP 2. 4
For each event, acquires a `std::sync::Mutex` protecting the shared `BounceFilter` state.
.IP 3. 4
Calls `BounceFilter::check_event`, which performs the debounce logic using timestamp comparisons based on data stored in fixed-size arrays within the `BounceFilter` struct (specifically, `last_event_us`). Updates the state if the event passes.
.IP 4. 4
Releases the mutex.
.IP 5. 4
Constructs an `EventInfo` struct containing the event and the filter result (passed/dropped, timings).
.IP 6. 4
Attempts to send the `EventInfo` to the logger thread via a bounded `crossbeam-channel` using `try_send`. If the channel is full, the message is dropped to avoid blocking.
.IP 7. 4
If the event was not dropped by the filter, writes the original `input_event` struct to standard output.
.PP
.B Logger Thread:
.IP 1. 4
Runs in a separate thread (`std::thread`).
.IP 2. 4
Waits to receive `EventInfo` messages from the main thread's channel, with a timeout to allow periodic checks.
.IP 3. 4
Processes received `EventInfo` messages:
.RS 4
.IP \(bu 4
Updates statistics stored in a `StatsCollector` struct. Simple counts use lock-free atomics (`std::sync::atomic`), while timing vectors (`Vec<u64>`) are updated directly as the `StatsCollector` is owned solely by the logger thread.
.IP \(bu 4
Performs logging to standard error based on the configured logging flags (`--log-all-events`, `--log-bounces`) and log level (`RUST_LOG`, `--verbose`). Logging uses the `tracing` framework.
.RE
.IP 4. 4
Periodically checks if the configured `--log-interval` has elapsed. If so, prints the current interval statistics and resets them.
.IP 5. 4
Continues until the main thread signals shutdown (by dropping the channel sender or setting an atomic flag).
.IP 6. 4
On shutdown, returns the final `StatsCollector` containing cumulative statistics to the main thread.
.PP
.B State Management:
.IP \(bu 4
The core filter state (`last_event_us` array) is protected by a `Mutex` to ensure safe access from the main thread (which modifies it) and potentially other threads if the design were different (though currently only the main thread accesses it mutably).
.IP \(bu 4
Statistics are managed primarily by the logger thread, minimizing contention on the main path.
.IP \(bu 4
Inter-thread communication uses `crossbeam-channel`, chosen for its performance characteristics.
"#;

/// Generates the man page with custom sections.
fn generate_man_page(cmd: &clap::Command, path: &Path) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    // Format date like 'Month Day, Year' e.g., "July 18, 2024"
    let date = chrono::Local::now().format("%B %d, %Y").to_string();
    let app_name_uppercase = cmd.get_name().to_uppercase();
    let bin_name = cmd.get_name();

    let mut buffer: Vec<u8> = Vec::new();

    // Render the standard sections (NAME, SYNOPSIS, DESCRIPTION, OPTIONS, AUTHOR) using clap_mangen
    // Note: clap_mangen uses the command's `about` for NAME and `long_about` (or `about`) for DESCRIPTION.
    // It doesn't include the .TH header automatically, so we add it manually first.
    writeln!(buffer, r#".TH "{}" 1 "{}" "{}" "User Commands""#, app_name_uppercase, date, version)?;
    Man::new(cmd.clone()).render(&mut buffer)?;

    // --- Append Custom Sections ---
    // These will appear *after* the standard sections generated above.
    let custom_sections = [
        // ("DESCRIPTION", MAN_DESCRIPTION), // clap_mangen handles DESCRIPTION based on cmd.about/long_about
        ("DEBOUNCING", MAN_DEBOUNCING), // How debounce works, choosing time
        ("NEAR-MISS", MAN_NEAR_MISS), // How near-miss works, interpretation
        ("EXAMPLES", MAN_EXAMPLES),
        ("INTEGRATION", MAN_INTEGRATION),
        ("STATISTICS", MAN_STATISTICS),
        ("LOGGING", MAN_LOGGING),
        ("SIGNALS", MAN_SIGNALS),
        ("THEORY OF OPERATION", MAN_THEORY_OF_OPERATION), // Internal architecture
        ("EXIT STATUS", MAN_EXIT_STATUS),
        ("PERFORMANCE", MAN_PERFORMANCE),
        ("TROUBLESHOOTING", MAN_TROUBLESHOOTING),
        ("ENVIRONMENT", MAN_ENVIRONMENT),
        ("BUGS", MAN_BUGS),
        ("SEE ALSO", MAN_SEE_ALSO),
    ];

    for (title, content_template) in custom_sections {
        writeln!(buffer, ".SH {}", title)?;
        // Format the content, replacing {bin_name} placeholder
        let formatted_content = content_template.replace("{bin_name}", bin_name);
        writeln!(buffer, "{}", formatted_content)?;
    }

    // Note: AUTHOR section is typically included by clap_mangen's render method.

    // Write the complete buffer (standard sections + custom sections) to the file
    fs::write(path, buffer).with_context(|| format!("Failed to write man page to {:?}", path))?;
    Ok(())
}

/// Generates shell completion files.
fn generate_completions(cmd: &clap::Command, completions_dir: &Path) -> Result<()> {
    let bin_name = cmd.get_name();
    // --- Generate Shell Completions ---
    let shells = [
        Shell::Bash,
        Shell::Elvish,
        Shell::Fish,
        Shell::PowerShell,
        Shell::Zsh,
    ];

    for shell in shells {
        let ext = match shell {
            Shell::Bash => "bash",
            Shell::Elvish => "elv",
            Shell::Fish => "fish",
            Shell::PowerShell => "ps1",
            Shell::Zsh => "zsh",
            _ => continue, // Should not happen
        };
        let completions_path = completions_dir.join(format!("{}.{}", bin_name, ext));
        println!("Generating completion file: {:?}", completions_path);
        let mut file = fs::File::create(&completions_path)
            .with_context(|| format!("Failed to create completion file: {:?}", completions_path))?;
        generate(shell, &mut cmd.clone(), bin_name.clone(), &mut file);
    }

    // --- Generate Nushell Completion ---
    let nu_path = completions_dir.join(format!("{}.nu", bin_name));
    println!("Generating Nushell completion file: {:?}", nu_path);
    let mut nu_file = fs::File::create(&nu_path)
        .with_context(|| format!("Failed to create Nushell completion file: {:?}", nu_path))?;
    generate(Nushell, &mut cmd.clone(), bin_name.clone(), &mut nu_file);

    Ok(())
}
