use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Simple wrapper for running cargo-xtask commands directly.
/// This allows running commands as `cargo xtask <cmd>` instead
/// of `cargo run --package xtask -- <cmd>`.
fn main() {
    // Skip the "cargo", "xtask" arguments that cargo adds
    let args: Vec<String> = env::args()
        .skip_while(|arg| arg != "xtask")
        .skip(1) // Skip "xtask" itself
        .collect();

    if args.is_empty() {
        println!(
            "Running xtask without arguments - use 'cargo xtask help' to see available commands"
        );
    }

    // Build path to the xtask binary
    let workspace_root = workspace_root();
    let xtask_path = workspace_root.join("target/debug/xtask");

    // Run cargo build if xtask binary doesn't exist
    if !xtask_path.exists() {
        println!("Building xtask first...");
        if !Command::new("cargo")
            .args(["build", "--package", "xtask"])
            .status()
            .expect("Failed to build xtask")
            .success()
        {
            eprintln!("Failed to build xtask");
            std::process::exit(1);
        }
    }

    // Run the xtask command
    let status = Command::new(xtask_path)
        .args(&args)
        .status()
        .expect("Failed to run xtask");

    std::process::exit(status.code().unwrap_or(1));
}

/// Find the workspace root.
fn workspace_root() -> PathBuf {
    let output = Command::new("cargo")
        .arg("locate-project")
        .arg("--workspace")
        .arg("--message-format=plain")
        .output()
        .expect("Failed to run cargo locate-project");

    let cargo_path = String::from_utf8(output.stdout).expect("Invalid UTF-8");
    PathBuf::from(cargo_path)
        .parent()
        .expect("Failed to get workspace root")
        .to_path_buf()
}
