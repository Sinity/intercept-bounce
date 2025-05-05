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
        Commands::GenerateDocs => generate_docs().context("Failed to 
generate docs"),                                                     
        Commands::Check => run_cargo("check", &[]).context("cargo    
check failed"),                                                      
        Commands::Test => run_cargo("test", &[]).context("cargo test 
failed"),                                                            
        Commands::Clippy => run_cargo("clippy", &["--", "-D",        
"warnings"])                                                         
            .context("cargo clippy failed"),                         
        Commands::FmtCheck => run_cargo("fmt", &["--",               
"--check"]).context("cargo fmt failed"),                             
    }                                                                
}                                                                    
                                                                     
fn run_cargo(command: &str, args: &[&str]) -> Result<()> {           
    let cargo = env::var("CARGO").unwrap_or_else(|_|                 
"cargo".to_string());                                                
    let mut cmd = Command::new(cargo);                               
    cmd.arg(command);                                                
    cmd.args(args);                                                  
    // Run in the workspace root                                     
    cmd.current_dir(project_root());                                 
                                                                     
    let status = cmd.status().context(format!("Failed to execute     
cargo {}", command))?;                                               
                                                                     
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
                                                                     
    fs::create_dir_all(&man_dir).context("Failed to create man       
directory")?;                                                        
    fs::create_dir_all(&completions_dir).context("Failed to create   
completions directory")?;                                            
                                                                     
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
Performance is critical for input filtering. \fB{bin_name}\fR uses a lock-free approach for the core filtering logic and a separate thread for logging and statistics to minimize latency impact on the main event processing path.
"#;

// Add more detail and context to examples
const MAN_EXAMPLES: &str = r#"
.PP
.B Basic Filtering (15ms window):
.IP
.nf
sudo sh \-c 'intercept \-g /dev/input/by\-id/your\-kbd | {bin_name} \-\-debounce\-time 15ms | uinput \-d /dev/input/by\-id/your\-kbd'
.fi
.PP
This command intercepts events from your keyboard (replace placeholder path), filters them with a 15ms debounce window, and creates a new virtual keyboard device with the filtered output. Applications should use the new virtual device created by \fBuinput\fR.
.PP
.B Filtering with Bounce Logging:
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-debounce\-time 20ms \-\-log\-bounces | uinput \-d ...'
.fi
.PP
Filter with a 20ms threshold and log only the key events that are dropped (considered bounces) to standard error. Useful for identifying which keys are chattering without logging every event.
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
.B Example udevmon.yaml Job:
.PP
This example demonstrates setting up \fB{bin_name}\fR to run automatically via \fBudevmon\fR whenever a specific keyboard is plugged in.
.IP
.nf
\- JOB: "intercept \-g $DEVNODE | {bin_name} \-\-debounce\-time 15ms | uinput \-d $DEVNODE"
  DEVICE:
    LINK: "/dev/input/by\-id/usb\-Your_Keyboard_Name\-event\-kbd"
.fi
.PP
Replace the \fILINK\fR value with the appropriate path for your keyboard found in \fI/dev/input/by-id/\fR. The \fI$DEVNODE\fR variable is automatically substituted by \fBudevmon\fR with the actual device path (e.g., /dev/input/event5).
.PP
Refer to the Interception Tools documentation for more details on configuring \fBudevmon\fR.
"#;

const MAN_STATISTICS: &str = r#"
\fB{bin_name}\fR collects and reports detailed statistics about the events it processes. These statistics include:
.IP \(bu 4
Overall counts (processed, passed, dropped)
.IP \(bu 4
Per-key drop counts and bounce timings (min/avg/max). Bounce time is the duration between a dropped event and the previous \fIpassed\fR event for the same key and state (press/release).
.IP \(bu 4
Near-miss events: Passed events that occurred just outside the debounce window but within the \fB\-\-near\-miss\-threshold\-time\fR of the previous passed event for the same key/state. Timings reported are the duration since the previous passed event. This helps identify keys close to bouncing.
.PP
Statistics are always printed to stderr on exit (cleanly or via signal). They can also be printed periodically using the \fB\-\-log\-interval\fR option.
.PP
The human-readable format includes percentages and formatted timings (µs, ms, s).
.PP
.B JSON Output (\-\-stats\-json):
.IP
Provides a machine-readable format containing all the same information as the human-readable output, plus configuration parameters used for the run. The structure includes overall counts, per-key drop statistics (nested under `press`, `release`, `repeat`), and near-miss timings. Timings are reported in microseconds.
"#;

const MAN_LOGGING: &str = r#"
\fB{bin_name}\fR provides several logging options for debugging and monitoring, written to standard error:
.IP \(bu 4
\fB\-\-log\-all\-events\fR: Log details of every incoming event to stderr (PASS or DROP)
.IP \(bu 4
\fB\-\-log\-bounces\fR: Log only dropped (bounced) key events to stderr
.IP \(bu 4
\fB\-\-verbose\fR: Enable verbose logging, including internal state and thread activity
.PP
Log messages include timestamps relative to the first event processed, event type/code/value, key names (if applicable), and bounce/near-miss timing information.
.PP
The \fBRUST_LOG\fR environment variable can be used to control log filtering (e.g., \fBRUST_LOG=debug\fR, \fBRUST_LOG=intercept_bounce=trace\fR). This overrides the default level set by \fB\-\-verbose\fR. See the \fBtracing_subscriber\fR documentation for filter syntax.
.PP
Note: Enabling \fB\-\-log\-all\-events\fR or high verbosity levels (trace) can impact performance due to the volume of log messages generated.
"#;

const MAN_SIGNALS: &str = r#"
\fB{bin_name}\fR handles the following signals gracefully:
.IP \(bu 4
SIGINT (Ctrl+C)
.IP \(bu 4
SIGTERM
.IP \(bu 4
SIGQUIT
.PP
When any of these signals are received, the program will shut down cleanly and print final statistics to stderr.
"#;

const MAN_EXIT_STATUS: &str = r#"
.IP 0 4
Success (including clean shutdown via signal).
.IP 1 4
General runtime error (e.g., I/O error reading/writing events, internal errors).
.IP 2 4
Error listing input devices when using the \fB\-\-list\-devices\fR option (e.g., permission denied).
"#;

const MAN_ENVIRONMENT: &str = r#"
.TP
.B RUST_LOG
Controls the logging verbosity and filtering. Examples: "info", "debug", "intercept_bounce=debug". Overrides the default log level implied by \fB\-\-verbose\fR.
"#;

const MAN_PERFORMANCE: &str = r#"
\fB{bin_name}\fR aims for minimal latency. The core filtering logic uses simple lookups and avoids locks. Logging and statistics are handled by a separate thread to avoid blocking the main event processing path.
.PP
Performance depends on the event rate from the input device and the system load. Enabling verbose logging (\fB\-\-log\-all\-events\fR or \fB\-\-verbose\fR with \fBRUST_LOG=trace\fR) can significantly increase CPU usage and potentially introduce latency due to contention writing to stderr.
.PP
Use \fBcargo bench\fR to run microbenchmarks for the core filter logic and inter-thread communication.
"#;

const MAN_BUGS: &str = r#"
Report bugs to: https://github.com/sinity/intercept-bounce/issues
"#;

const MAN_SEE_ALSO: &str = r#"
\fBintercept\fR(1), \fBuinput\fR(1), \fBudevmon\fR(1), \fBinput_event\fR(5)
.PP
Full documentation at: https://github.com/sinity/intercept-bounce
"#;

const MAN_TROUBLESHOOTING: &str = r#"
.TP
.B Permission Denied:
Running \fBintercept\fR and \fBuinput\fR typically requires root privileges or specific user group memberships (e.g., 'input'). Ensure the user running the pipeline has read access to the input device (\fI/dev/input/event*\fR) and write access to \fI/dev/uinput\fR.
.TP
.B Incorrect Device Path:
Double-check the device path used with \fBintercept \-g\fR or in \fIudevmon.yaml\fR. Use paths from \fI/dev/input/by-id/\fR for stability.
.TP
.B Mixed Output in Terminal:
If running interactively with logging enabled, log messages (stderr) might mix with terminal echo or shell output. Redirect stderr (\fI2> logfile.txt\fR) or use \fBudevmon\fR for background operation.
"#;

/// Generates the man page with custom sections.
fn generate_man_page(cmd: &clap::Command, path: &Path) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    // Format date like 'Month Day, Year' e.g., "July 18, 2024"
    let date = chrono::Local::now().format("%B %d, %Y").to_string();
    let app_name_uppercase = cmd.get_name().to_uppercase();
    let bin_name = cmd.get_name();

    let mut buffer: Vec<u8> = Vec::new();

    // --- Header ---
    writeln!(buffer, r#".TH "{}" 1 "{}" "{}" "User Commands""#, app_name_uppercase, date, version)?;

    // --- NAME ---
    writeln!(buffer, ".SH NAME")?;
    writeln!(buffer, r#"{} \- {}"#, bin_name, cmd.get_about().unwrap_or_default().replace('-', r"\-"))?;

    // --- SYNOPSIS ---
    writeln!(buffer, ".SH SYNOPSIS")?;
    writeln!(buffer, r#".B {}"#, bin_name)?;
    writeln!(buffer, r#" [OPTIONS]"#)?;

    // --- OPTIONS (Generated by clap_mangen) ---
    writeln!(buffer, ".SH OPTIONS")?;
    Man::new(cmd.clone()).render_section_into("OPTIONS", &mut buffer)?;

    // --- Custom Sections ---
    let sections = [
        ("DESCRIPTION", MAN_DESCRIPTION),
        ("EXAMPLES", MAN_EXAMPLES),
        ("INTEGRATION", MAN_INTEGRATION),
        ("STATISTICS", MAN_STATISTICS),
        ("LOGGING", MAN_LOGGING),
        ("SIGNALS", MAN_SIGNALS),
        ("EXIT STATUS", MAN_EXIT_STATUS),
        ("PERFORMANCE", MAN_PERFORMANCE),
        ("TROUBLESHOOTING", MAN_TROUBLESHOOTING),
        ("ENVIRONMENT", MAN_ENVIRONMENT),
        ("BUGS", MAN_BUGS),
        ("SEE ALSO", MAN_SEE_ALSO),
    ];

    for (title, content_template) in sections {
        writeln!(buffer, ".SH {}", title)?;
        // Format the content, replacing {bin_name} placeholder
        let formatted_content = content_template.replace("{bin_name}", bin_name);
        writeln!(buffer, "{}", formatted_content)?;
    }

    // --- AUTHOR ---
    writeln!(buffer, ".SH AUTHOR")?;
    writeln!(buffer, r#"Written by {}."#, cmd.get_author().unwrap_or("Unknown"))?;

    // Write the buffer to the file
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
        generate(shell, &mut cmd.clone(), &bin_name, &mut file);     
    }                                                                
                                                                     
    // --- Generate Nushell Completion ---                           
    let nu_path = completions_dir.join(format!("{}.nu", bin_name));  
    println!("Generating Nushell completion file: {:?}", nu_path);   
    let mut nu_file = fs::File::create(&nu_path)                     
        .with_context(|| format!("Failed to create Nushell completion file: {:?}", nu_path))?;                                             
    generate(Nushell, &mut cmd.clone(), &bin_name, &mut nu_file);
    
    Ok(())
}
          
