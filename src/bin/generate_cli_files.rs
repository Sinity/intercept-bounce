// src/bin/generate_cli_files.rs
use clap::CommandFactory;
use clap_complete_nushell::Nushell; // Moved import to top
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
    // Import items needed for standard shell generation
    use clap_complete::{generate, Shell};

    // Define shells to generate completions for
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
            _ => continue, // Should not happen with explicit list
        };
        let completions_path = out_path.join(format!("{bin_name}.{ext}"));
        println!("Generating completion file: {completions_path:?}");
        // Explicitly create the file first.
        let mut file = fs::File::create(&completions_path)?;
        // Call generate with the file handle (which implements Write).
        generate(shell, &mut cmd.clone(), bin_name, &mut file);
    }

    // --- Generate Nushell Completion ---
    // Nushell generator is imported at the top now
    let nu_path = out_path.join(format!("{bin_name}.nu"));
    println!("Generating Nushell completion file: {nu_path:?}");
    let mut nu_file = fs::File::create(&nu_path)?;
    // Use the generate function from clap_complete with the Nushell generator struct
    clap_complete::generate(Nushell, &mut cmd.clone(), bin_name, &mut nu_file);

    println!(
        "Successfully generated man page and completions in: {}",
        out_path.display()
    );
    Ok(())
}
