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
    let man_file = fs::File::create(&man_path)?;
    println!("Generating man page: {:?}", man_path);
    Man::new(cmd.clone()).render(man_file)?;

    // --- Generate Shell Completions ---
    let bin_name = "intercept-bounce"; // Your binary name
    for shell in Shell::value_variants() {
        let completions_path = out_path.join(format!("{}.{}", bin_name, shell));
        println!("Generating completion file: {:?}", completions_path);
        generate_to(*shell, &mut cmd.clone(), bin_name, &out_path)?;
    }

    println!(
        "Successfully generated man page and completions in: {}",
        out_path.display()
    );
    Ok(())
}

