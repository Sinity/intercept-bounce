# Repository Guidelines

## Project Structure & Module Organization
The runtime crate lives under `src/`, where `main.rs` wires the CLI in `cli.rs` to the filtering core in `filter/`. Shared utilities (`config.rs`, `telemetry.rs`, `logger.rs`) keep side-effects isolated. Integration and property tests sit in `tests/`, with reusable fixtures in `test-helpers/`. Performance work belongs in `benches/`, while `fuzz/` houses `cargo fuzz` targets. Reference material and generated manuals land in `docs/`, and the `xtask/` helper orchestrates doc builds.

## Build, Test, and Development Commands
- `cargo build --release` compiles the filter binary with production optimizations.
- `cargo test --all` runs unit, integration, and property suites; mirror CI before opening a PR.
- `cargo clippy --all-targets -- -D warnings` enforces lint cleanliness; match CI expectations.
- `cargo fmt --all` or `./dev.sh fmt` keeps formatting consistent; run before committing.
- `./dev.sh all` chains fmt, clippy, and tests for a quick gate.
- `cargo run --package xtask --bin xtask -- generate-docs` refreshes the CLI/manpage material in `docs/`.
- `nix develop` drops you into the flake-provisioned dev shell; use `./dev.sh nix` for a full build.

## Coding Style & Naming Conventions
Rust code follows standard 4-space indentation and `rustfmt` defaults. Modules, files, and test helpers use `snake_case`; types remain `PascalCase`. Prefer explicit `use crate::...` paths for clarity, and document non-obvious helpers with `///` comments. CI treats clippy warnings as errors, so resolve lints locally.

## Runtime Configuration Tips
- `--ignore-key <KEY>` lets you exempt specific controls (e.g., `KEY_VOLUMEDOWN` encoder wheels) from debouncing; the flag accepts symbolic names or numeric codes.
- Keep `--log-bounces` on when developing input pipelines so freshly ignored keys can be verified quickly via the systemd journal.

## Testing Guidelines
Keep fast checks in `tests/` and push slower fuzz targets under `fuzz/`. Name new integration files `*_tests.rs`; within modules, group cases under `mod tests { ... }`. Run `cargo test --all` before pushing, and refresh relevant property tests when touching timing logic. For fuzzing, seed with `cargo fuzz run fuzz_core_filter` or `fuzz_target_stats` and commit any minimized corpus updates. Benchmarks live in `benches/filter.rs`; use `cargo bench` to validate performance-sensitive changes.

## Commit & Pull Request Guidelines
History favors concise, imperative subjects (e.g., `Tighten near-miss logging`); include a brief body when context is not obvious. Squash fixups locally so each commit passes `./dev.sh all`. PRs should link issues when applicable, summarize behavioural impact, and cite how you validated the change. Attach log snippets or screenshots for user-visible output shifts, and call out configuration updates in `docs/` or sample pipelines.
