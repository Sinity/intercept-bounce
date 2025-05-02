fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Emit the instructions needed to capture git revision, build timestamp, etc.
    vergen::EmitBuilder::builder()
        .git_commit_timestamp()
        .git_sha(true) // Generate GIT_SHA and RUSTC_SEMVER
        .build_timestamp() // Generate BUILD_TIMESTAMP
        .emit()?;
    Ok(())
}
