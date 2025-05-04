// src/bin/generate_cli_files.rs
use clap::{CommandFactory, ValueEnum};
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;
use std::{env, fs, io::Error, path::Path};

// Import the Args struct from the library crate
use intercept_bounce::cli::Args;

fn main() -> Result<(), Error> {
    // Generate completions and man page based on environment variable during build
    // Example: OUT_DIR=target/generated cargo run --bin generate-cli-files
    let outdir = env::var_os("OUT_DIR").unwrap_or_else(|| "target/generated".into());
    let out_path = Path::new(&outdir);
    fs::create_dir_all(out_path)?;

    let cmd = Args::command(); // Get the clap::Command struct

    // --- Generate Man Page ---
    let man_path = out_path.join("intercept-bounce.1");
    let mut man_file = fs::File::create(&man_path)?; // Make man_file mutable
    println!("Generating man page: {man_path:?}"); // Use inline formatting
    Man::new(cmd.clone()).render(&mut man_file)?; // Pass a mutable reference

    // --- Generate Shell Completions ---
    let bin_name = "intercept-bounce"; // Your binary name
    // Generate for supported shells explicitly
    for shell in [
        Shell::Bash,
        Shell::Elvish,
        Shell::Fish,
        Shell::PowerShell,
        Shell::Zsh,
    ] {
        let ext = match shell {
            Shell::Bash => "bash",
            Shell::Elvish => "elv",
            Shell::Fish => "fish",
            Shell::PowerShell => "ps1",
            Shell::Zsh => "zsh",
            _ => continue, // Skip unknown/unsupported shells if any appear in future versions
        };
        let completions_path = out_path.join(format!("{bin_name}.{ext}"));
        println!("Generating completion file: {completions_path:?}");
        // Generate directly to the final path, ensuring the target directory exists
        if let Some(parent) = completions_path.parent() {
             fs::create_dir_all(parent)?;
        }
        generate_to(shell, &mut cmd.clone(), bin_name, &completions_path)?;
    }

    println!(
        "Successfully generated man page and completions in: {}",
        out_path.display()
    );
    Ok(())
}
