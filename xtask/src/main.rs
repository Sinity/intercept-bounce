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

const MAN_DESCRIPTION: &str = r#"
\fB{bin_name}\fR is an Interception Tools filter designed to eliminate keyboard chatter (also known as switch bounce).
It reads Linux \fBinput_event\fR(5) structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output.
Statistics are printed to standard error on exit or periodically.

This is particularly useful for mechanical keyboards which can sometimes register multiple presses or releases for a single physical key action due to noisy switch contacts.
It integrates with the Interception Tools ecosystem, typically placed in a pipeline between \fBintercept\fR(1) and \fBuinput\fR(1).
"#;

const MAN_EXAMPLES: &str = r#"
.PP
.B Basic Filtering (15ms window):
.IP
.nf
sudo sh \-c 'intercept \-g /dev/input/by\-id/your\-kbd | {bin_name} \-\-debounce\-time 15ms | uinput \-d /dev/input/by\-id/your\-kbd'
.fi
.PP
.B Filtering with Bounce Logging:
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-debounce\-time 20ms \-\-log\-bounces | uinput \-d ...'
.fi
.PP
.B Periodic Stats Dump (every 5 minutes):
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-log\-interval 5m | uinput \-d ...'
.fi
.PP
.B JSON Statistics Output:
.IP
.nf
sudo sh \-c 'intercept \-g ... | {bin_name} \-\-stats\-json | uinput \-d ...' > /dev/null
.fi
"#;

const MAN_INTEGRATION: &str = r#"
\fB{bin_name}\fR is designed to work with Interception Tools. It can be used in pipelines or within a \fBudevmon\fR(1) configuration file (\fIudevmon.yaml\fR).
.PP
.B Example udevmon.yaml Job:
.IP
.nf
\- JOB: "intercept \-g $DEVNODE | {bin_name} \-\-debounce\-time 15ms | uinput \-d $DEVNODE"
  DEVICE:
    LINK: "/dev/input/by\-id/usb\-Your_Keyboard_Name\-event\-kbd"
.fi
"#;

const MAN_STATISTICS: &str = r#"
\fB{bin_name}\fR collects and reports detailed statistics about the events it processes. These statistics include:
.IP \(bu 4
Overall counts (processed, passed, dropped)
.IP \(bu 4
Per-key drop counts and bounce timings (min/avg/max)
.IP \(bu 4
Near-miss events that occur just outside the debounce window
.PP
Statistics are always printed to stderr on exit (cleanly or via signal). They can also be printed periodically using the \fB\-\-log\-interval\fR option.
.PP
When using \fB\-\-stats\-json\fR, statistics are output in JSON format for easier parsing and integration with monitoring tools.
"#;

const MAN_LOGGING: &str = r#"
\fB{bin_name}\fR provides several logging options for debugging and monitoring:
.IP \(bu 4
\fB\-\-log\-all\-events\fR: Log details of every incoming event to stderr (PASS or DROP)
.IP \(bu 4
\fB\-\-log\-bounces\fR: Log only dropped (bounced) key events to stderr
.IP \(bu 4
\fB\-\-verbose\fR: Enable verbose logging, including internal state and thread activity
.PP
The RUST_LOG environment variable can be used to control log filtering (e.g., RUST_LOG=debug).
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
\fB{bin_name}\fR exits with status 0 on success, 1 on error, and 2 on device listing errors.
"#;

const MAN_ENVIRONMENT: &str = r#"
.TP
.B RUST_LOG
Controls the logging verbosity and filtering. Examples: "info", "debug", "intercept_bounce=debug".
"#;

const MAN_BUGS: &str = r#"
Report bugs to: https://github.com/sinity/intercept-bounce/issues
"#;

const MAN_SEE_ALSO: &str = r#"
\fBintercept\fR(1), \fBuinput\fR(1), \fBudevmon\fR(1), \fBinput_event\fR(5)
.PP
Full documentation at: https://github.com/sinity/intercept-bounce
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
          
