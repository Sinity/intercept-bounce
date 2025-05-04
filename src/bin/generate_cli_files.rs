// src/bin/generate_cli_files.rs
use clap::CommandFactory;
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;
use std::{env, fs, io::Error, path::Path};

// Import the Args struct from the library crate
use intercept_bounce::cli::Args;

fn main() -> Result<(), Error> {
    // Get output directory from environment variable or default.
    let outdir = env::var_os("OUT_DIR").unwrap_or_else(|| "target/generated".into());
    let out_path = Path::new(&outdir);
    fs::create_dir_all(out_path)?;

    let cmd = Args::command();

    // --- Generate Man Page ---
    let man_path = out_path.join("intercept-bounce.1");
    let mut man_file = fs::File::create(&man_path)?;
    println!("Generating man page: {man_path:?}");
    Man::new(cmd.clone()).render(&mut man_file)?;

    // --- Generate Shell Completions ---
    let bin_name = "intercept-bounce";
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
            _ => continue, // Should not happen with explicit list
        };
        let completions_path = out_path.join(format!("{bin_name}.{ext}"));
        println!("Generating completion file: {completions_path:?}");
        // The output directory `out_path` is already created before the loop.
        // `generate_to` writes directly to the file path provided.
        generate_to(shell, &mut cmd.clone(), bin_name, &completions_path)?;
    }

    println!(
        "Successfully generated man page and completions in: {}",
        out_path.display()
    );
    Ok(())
}
