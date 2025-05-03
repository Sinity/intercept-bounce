fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Emit the instructions needed to capture git revision, build timestamp, etc.
    vergen::EmitBuilder::builder()
        .all_build() // Emit all build-related instructions (timestamp, rustc, etc.)
        .all_git() // Emit all git-related instructions (sha, commit timestamp, etc.)
        .emit()?;
    Ok(())
}
