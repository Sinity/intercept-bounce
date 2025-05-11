use vergen::EmitBuilder;

// Simple build script that just generates build information
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Generate build info using vergen for version and build metadata
    EmitBuilder::builder()
        .all_build() // Emit build-related instructions (timestamp, rustc, etc.)
        .all_git() // Emit git-related instructions (sha, commit timestamp, etc.)
        .emit()?;

    // Note: Documentation is now generated explicitly via `cargo xtask docs`
    // rather than during the build process

    Ok(())
}
