# intercept-bounce

[![Crates.io](https://img.shields.io/crates/v/intercept-bounce.svg)](https://crates.io/crates/intercept-bounce)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://github.com/sinity/intercept-bounce/actions/workflows/ci.yml/badge.svg)](https://github.com/sinity/intercept-bounce/actions/workflows/ci.yml)

An [Interception Tools](https://gitlab.com/interception/linux/tools) filter designed to eliminate keyboard chatter (also known as switch bounce) while providing detailed statistics and diagnostics.

It reads raw Linux `input_event` structs from standard input, filters out rapid duplicate key press/release events based on a configurable time threshold, and writes the filtered events to standard output. Comprehensive statistics about dropped events, timings, and near-misses are printed to standard error on exit or periodically.

## Features

* **Configurable Debouncing:** Filters rapid duplicate key press/release events within a specified time window (`--debounce-time`, default: 25ms). Key repeats (value=2) are never filtered.
* **Near-Miss Tracking:** Identifies and reports key events that *pass* the filter but occur just slightly after the debounce window closes (`--near-miss-threshold-time`, default: 100ms). Useful for diagnosing keys with inconsistent timing.
* **Detailed Statistics (Human-Readable & JSON):**
  * Overall counts (key events processed, passed, dropped).
  * Overall histograms showing the distribution of bounce and near-miss timings.
  * Per-key statistics:
    * Total processed, passed, dropped counts, and drop rate (%).
    * Bounce time statistics (min/avg/max) for dropped press/release events.
    * Near-miss time statistics (min/avg/max) for passed press/release events.
    * Detailed histograms for bounce and near-miss timings per key/state (JSON only).
* **Flexible Logging:**
  * `--log-all-events`: Log details ([PASS]/[DROP]) for (almost) every event.
  * `--log-bounces`: Log details only for dropped (bounced) key events.
  * `--verbose`: Enable DEBUG level internal logging.
  * `RUST_LOG` environment variable for fine-grained `tracing` filter control (overrides `--verbose`).
* **Per-Key Controls:** Use `--debounce-key KEY_ENTER` to limit debouncing to specific keys (multiple instances allowed), or `--ignore-key KEY_VOLUMEDOWN` to exempt controls entirely; both accept names or numeric codes. If a key appears in both lists, `--debounce-key` wins so the key is still debounced.
* **Periodic Reporting:** Dump statistics periodically (`--log-interval`, default: 15m).
* **JSON Output:** Output statistics in JSON format (`--stats-json`) for machine parsing.
* **Graceful Shutdown:** Handles SIGINT, SIGTERM, SIGQUIT to ensure final statistics are reported.
* **Device Listing:** List available input devices with keyboard capabilities (`--list-devices`).
* **Debugging Ring Buffer:** Optionally store the last N passed events in memory for debugging complex issues (`--ring-buffer-size`).
* **OpenTelemetry Export:** Optionally export metrics to an OTLP endpoint (`--otel-endpoint`).
* **Interception Tools Integration:** Designed for use in standard Interception Tools pipelines (`intercept | intercept-bounce | uinput`).
* **Robust Testing:** Includes unit tests, integration tests (`assert_cmd`), property tests (`proptest`), and fuzzing (`cargo-fuzz`).
* **Benchmarking:** Core filter logic and channel communication can be benchmarked (`cargo bench`).

## Installation

### From Crates.io

```bash
cargo install intercept-bounce
```

### From Source (Git Repository)

```bash
git clone https://github.com/sinity/intercept-bounce.git
cd intercept-bounce
cargo install --path .
```

### Using Nix

With Nix flakes enabled:

```bash
# Build and run directly
nix run github:sinity/intercept-bounce -- --help

# Build the package
nix build github:sinity/intercept-bounce

# Install into your Nix profile
nix profile install github:sinity/intercept-bounce
```

#### NixOS System (Declarative, using flake)

Since `intercept-bounce` is not yet packaged in the official `nixpkgs` repository, the recommended way to install it declaratively on NixOS is by adding this repository as a flake input to your system or Home Manager configuration.

1.  **Add the flake input:**
    Modify your top-level `flake.nix` (e.g., `/etc/nixos/flake.nix` or `~/.config/home-manager/flake.nix`) to include `intercept-bounce`:

    ```nix
    # In your flake.nix
    {
      description = "Your system configuration";

      inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable"; # Or your preferred channel

        # Add intercept-bounce flake input
        intercept-bounce.url = "github:sinity/intercept-bounce";
        # Optional: Pin to a specific commit or tag for stability
        # intercept-bounce.inputs.nixpkgs.follows = "nixpkgs"; # Ensure it uses your nixpkgs
        # intercept-bounce.rev = "YOUR_COMMIT_HASH_HERE";
      };

      outputs = { self, nixpkgs, intercept-bounce, ... }@inputs:
        let
          system = "x86_64-linux"; # Or your system architecture
          pkgs = import nixpkgs { inherit system; };
        in
        {
          # Example for configuration.nix:
          nixosConfigurations.your-hostname = nixpkgs.lib.nixosSystem {
            inherit system;
            specialArgs = { inherit inputs; }; # Pass inputs to your config module
            modules = [
              ./configuration.nix # Your main configuration file
              # ... other modules
            ];
          };

          # Example for Home Manager (within a Home Manager flake):
          # homeConfigurations."your-username" = home-manager.lib.homeManagerConfiguration {
          #   inherit pkgs;
          #   extraSpecialArgs = { inherit inputs; }; # Pass inputs to your config module
          #   modules = [
          #     ./home.nix # Your main Home Manager config file
          #     # ... other modules
          #   ];
          # };
        };
    }
    ```

#### NixOS module

This repository also ships a reusable NixOS module that renders the CLI invocation, exposes every flag as a typed option, and (optionally) adds the executable to `environment.systemPackages`. After adding the flake input, enable the module and tune the options that matter to your keyboard pipeline:

```nix
{
  imports = [ inputs.intercept-bounce.nixosModules.default ];

  services.interceptBounce = {
    enable = true;
    debounceTime = "40ms";
    logInterval = "6h";
    logBounces = true;
    statsJson = true;
    extraArgs = [ "--debounce-key" "KEY_ENTER" ];
  };
}
```

The module exposes the resolved command in both list (`services.interceptBounce.command`) and string (`services.interceptBounce.commandString`) form, making it straightforward to slot into `udevmon` pipelines or higher-level abstractions.

2.  **Add the package to your configuration:**
    In the NixOS or Home Manager module file referenced above (e.g., `configuration.nix` or `home.nix`), add the package to your desired list, referencing it via the `inputs`:

    ```nix
    # In /etc/nixos/configuration.nix or ~/.config/home-manager/home.nix
    { pkgs, inputs, ... }: # Ensure 'inputs' is available here

    {
      # Example for NixOS system packages:
      environment.systemPackages = with pkgs; [
        # ... other packages

        # Reference the default package from the intercept-bounce flake input
        inputs.intercept-bounce.packages.${pkgs.system}.default
      ];

      # Example for Home Manager packages:
      # home.packages = with pkgs; [
      #   inputs.intercept-bounce.packages.${pkgs.system}.default
      # ];

      # ... rest of your configuration
    }
    ```

3.  **Rebuild your configuration:**
    Apply the changes using `sudo nixos-rebuild switch` (for NixOS) or `home-manager switch` (for Home Manager).

#### User Profile (Non-NixOS or User-Local)

If you are not using NixOS or prefer to install the tool only for your user, you can install it directly into your Nix profile from the flake URL:

```bash
nix profile install github:sinity/intercept-bounce
```

This command is declarative for your user profile but does not integrate the package into the NixOS system configuration.

#### Building and Running Directly

You can also build or run the package directly without installing it permanently:

```bash
# Build the package (output in ./result)
nix build github:sinity/intercept-bounce

# Run directly without installing
nix run github:sinity/intercept-bounce -- --help
```

The Nix flake also provides a development shell (`nix develop`) with necessary tools (see [Development](#development)).

## Usage

`intercept-bounce` is designed to be used within an Interception Tools pipeline.

### Basic Pipeline

The most common usage involves capturing events from a physical keyboard, filtering them with `intercept-bounce`, and creating a new virtual keyboard with the filtered output using `uinput`.

```bash
# Find your keyboard device first (e.g., using 'intercept-bounce --list-devices' or 'intercept -L')
# Example device path: /dev/input/by-id/usb-My_Awesome_Keyboard-event-kbd

# Run the pipeline (requires root/sudo)
sudo sh -c 'intercept -g /dev/input/by-id/usb-My_Awesome_Keyboard-event-kbd \
           | intercept-bounce --debounce-time 15ms \
           | uinput -d /dev/input/by-id/usb-My_Awesome_Keyboard-event-kbd'
# Note: Using the virtual device created by uinput requires configuration. See the "Integration" section below.
```

**Important:**

1. Replace `/dev/input/by-id/usb-My_Awesome_Keyboard-event-kbd` with the actual path to **your** keyboard device. Using paths from `/dev/input/by-id/` is recommended as they are stable.
2. The device path provided to `intercept -g` **must** be the same as the one provided to `uinput -d`.
3. This command creates a **new virtual device**. Your desktop environment (Xorg/Wayland) needs to use this new device instead of the original physical one. See the [Integration](#integration-with-interception-tools) section for details.

#### Filtering Only Certain Keys

Use `--debounce-key` when you only want chatter protection on a handful of controls. Any keys not listed pass straight through.

```bash
sudo sh -c 'intercept -g $DEVNODE \
           | intercept-bounce --debounce-time 100ms \
                               --debounce-key KEY_ENTER \
                               --debounce-key KEY_SPACE \
           | uinput -d $DEVNODE'
```

You can still supply `--ignore-key` for the allowlisted set—`--debounce-key` wins if both flags mention the same code—so it’s safe to keep shared configs that exempt volume wheels without losing an explicit per-key allowlist.

### udevmon Integration (Recommended)

Using `udevmon` (part of Interception Tools) is the recommended way to manage the pipeline automatically when the device is connected/disconnected. Add a job to your `/etc/interception/udevmon.yaml` (or user-specific config):

```yaml
- JOB: intercept -g $DEVNODE | intercept-bounce --debounce-time 15ms | uinput -d $DEVNODE
  DEVICE:
    LINK: /dev/input/by-id/usb-My_Awesome_Keyboard-event-kbd # <-- Change this!
```

Remember to replace the `LINK` with the correct path for your keyboard and restart the `udevmon` service (`sudo systemctl restart interception-udevmon` or similar).

## Command-Line Options

```
Usage: intercept-bounce [OPTIONS]

Options:
  -t, --debounce-time <DURATION>
          Debounce time threshold (e.g., "25ms", "0.01s"). [default: 25ms]
      --near-miss-threshold-time <DURATION>
          Threshold for logging "near-miss" events (e.g., "100ms"). [default: 100ms]
      --log-interval <DURATION>
          Periodically dump statistics to stderr (e.g., "15m", "60s", "0s" to disable). [default: 15m]
      --log-all-events
          Log details of *every* incoming event ([PASS]/[DROP]).
      --log-bounces
          Log details of *only dropped* (bounced) key events.
      --list-devices
          List available input devices and their capabilities (requires root).
      --stats-json
          Output statistics as JSON format to stderr.
      --verbose
          Enable verbose logging (DEBUG level).
      --ring-buffer-size <SIZE>
          Size of the ring buffer for storing recently passed events (0 to disable). [default: 0]
      --debounce-key <KEY>
          Key codes or names to debounce. When present, only these keys are debounced (all others pass through). Repeat the flag to list multiple keys.
      --ignore-key <KEY>
          Key codes or names to never debounce unless they are also provided via `--debounce-key`.
      --otel-endpoint <URL>
          OTLP endpoint URL for exporting traces and metrics (e.g., "http://localhost:4317").
  -h, --help
          Print help
  -V, --version
          Print version
```

For detailed explanations of each option, see `man intercept-bounce` (if installed) or `intercept-bounce --help`.

## How it Works

### Debouncing

`intercept-bounce` filters key chatter by remembering the timestamp of the last *passed* event for each unique combination of key code (e.g., `KEY_A`) and key state (press=1, release=0).

1. When a new key event arrives, its timestamp is compared to the last passed timestamp for the *same key code and state*.
2. If the time difference is *less than* the configured `--debounce-time`, the new event is considered a bounce and is **dropped**.
3. If the time difference is *greater than or equal to* the `--debounce-time`, or if the event has a different key code or state, the event is **passed** through, and its timestamp becomes the new "last passed" time for that specific key/state.
4. Key repeat events (value=2) are **always passed** without debouncing.
5. Non-key events (mouse, sync, etc.) are **always passed**.

### Near-Miss Tracking

This feature helps diagnose keys with inconsistent timing just outside the debounce window.

1. When a key event *passes* the debounce filter, the time difference since the *previous passed event* for the same key/state is calculated.
2. If this difference is *less than or equal to* the `--near-miss-threshold-time`, the event is recorded as a "near-miss" in the statistics.
3. High near-miss counts for a key might indicate a failing switch or that the `--debounce-time` needs adjustment.

## Statistics

`intercept-bounce` collects detailed statistics, printed to `stderr` on exit (Ctrl+C) or periodically (`--log-interval`).

### Human-Readable Format (Default)

* **Overall Statistics:** Total key events processed, passed, dropped, and overall drop percentage.
* **Overall Histograms:** Visual distribution of bounce timings and near-miss timings across all keys.
* **Dropped Event Statistics Per Key:** For each key with activity:
  * Summary: Total processed, passed, dropped, drop %.
  * Details per state (Press/Release/Repeat): Processed, Passed, Dropped, Drop Rate (%), Bounce Time (Min/Avg/Max) if drops occurred.
* **Passed Event Near-Miss Statistics:** For each key/state with near-misses: Count, Near-Miss Time (Min/Avg/Max).

### JSON Format (`--stats-json`)

Provides a machine-readable JSON object containing all the information from the human-readable report, plus raw timing data arrays and detailed histogram bucket counts. Key top-level fields:

* `report_type`: "Cumulative" or "Periodic".
* `runtime_us`: Total runtime (cumulative only).
* Configuration values (`debounce_time_us`, `near_miss_threshold_us`, etc.).
* Overall counts (`key_events_processed`, `key_events_passed`, `key_events_dropped`).
* `overall_bounce_histogram`, `overall_near_miss_histogram`: Detailed histogram objects.
* `per_key_stats`: Array of objects per key, including detailed stats per state (press/release/repeat) with sampled `timings_us`, `min_us`/`max_us`/`avg_us`, and a `bounce_histogram`.
* `per_key_near_miss_stats`: Array of objects per key/state with sampled `timings_us`, summary fields, and a `near_miss_histogram`.
  Sample arrays retain only the most recent timings to avoid unbounded memory growth.

Refer to the `StatsCollector::print_stats_json` implementation or the man page for the exact structure.

### Histograms

Histograms show the distribution of timings (bounce or near-miss) in milliseconds across predefined buckets (e.g., `<1ms`, `1-2ms`, `2-4ms`, ..., `>=128ms`). They help visualize the typical duration of bounces or near-misses. The average timing is also calculated.

## Logging

Logging messages are printed to `stderr`.

* `--log-all-events`: Logs `[PASS]` or `[DROP]` for almost every event, showing type, code, value, key name, and timing info. (Skips `EV_SYN`/`EV_MSC` for clarity). **Performance impact!**
* `--log-bounces`: Logs only `[DROP]` messages for key events, including bounce time. Less verbose than `--log-all-events`.
* `--verbose`: Enables `DEBUG` level logging, showing internal state, thread activity, etc. Sets default filter to `intercept_bounce=debug` if `RUST_LOG` is not set.
* **`RUST_LOG` Environment Variable:** Provides fine-grained control using the `tracing_subscriber::EnvFilter` format (e.g., `RUST_LOG=info`, `RUST_LOG=intercept_bounce=trace`, `RUST_LOG=warn,intercept_bounce::filter=debug`). **Overrides** `--verbose`.

**Performance Note:** High logging verbosity (`--log-all-events`, `RUST_LOG=trace`) can significantly impact performance and may cause log messages to be dropped if the logger thread cannot keep up. A warning ("Logger channel full...") will be printed if this happens.

## Integration with Interception Tools

* **Pipeline:** The standard usage is `intercept -g <device> | intercept-bounce [OPTIONS] | uinput -d <device>`.
* **udevmon:** Recommended for managing the pipeline automatically. See [Usage](#udevmon-integration-recommended).
* **Virtual Device:** `uinput` creates a *new* virtual input device (e.g., `/dev/input/eventX`). Your Desktop Environment (Xorg/Wayland) **must** be configured to use this new virtual device. The original physical device still emits raw events. Configuration methods vary; sometimes automatic, sometimes requiring DE-specific settings (e.g., Xorg `InputClass` sections, Wayland compositor settings). Use tools like `libinput list-devices` to identify the virtual device (often contains "Uinput" or "intercept-bounce" in the name).
* **Wayland/Xorg:** Interception Tools generally work more reliably under Xorg. Wayland compositors often restrict global input grabbing. Using `intercept-bounce` under Wayland might require specific compositor support or configuration to recognize and prioritize the `uinput` virtual device.

## Troubleshooting

* **Permission Denied:** Running `intercept` and `uinput` requires root privileges or specific group memberships (`input` group for reading `/dev/input/event*`, potentially custom udev rules for `/dev/uinput` write access). Using `sudo sh -c '...'` for the whole pipeline is common. `intercept-bounce --list-devices` also needs read access.
* **Incorrect Device Path:** Ensure the path used for `intercept -g` and `uinput -d` is identical and correct. Use stable paths from `/dev/input/by-id/`. Use `intercept-bounce --list-devices` or `intercept -L` to find devices.
* **Filter Not Working / No Output:**
  * Check pipeline order and permissions.
  * Verify device paths match.
  * Check `udevmon` status and logs (`sudo systemctl status interception-udevmon`, `journalctl -u interception-udevmon`).
  * Run `intercept-bounce` with `--verbose` or `--log-all-events` to check processing and stderr for errors.
  * Confirm your DE is using the virtual device created by `uinput`.
* **Too Much Filtering (Missed Keystrokes):** Lower `--debounce-time`.
* **Too Little Filtering (Chatter Still Occurs):** Increase `--debounce-time`. Use `--log-bounces` or statistics (bounce timings/histograms) with a low debounce time first to measure the chatter duration, then set the time slightly higher.
* **Mixed Output in Terminal:** Redirect stderr (`2> log.txt`) or use `udevmon`.
* **"Logger channel full..." Warning:** Logger thread can't keep up (heavy logging, slow OTLP endpoint, high load). Log messages/stats may be lost. Reduce logging verbosity or disable OTLP if problematic.
* **JSON Stats Errors:** Check stderr for non-JSON error messages printed before the JSON output.

## Development

### Building

```bash
cargo build
cargo build --release
```

### Testing

```bash
# Run all tests (unit, integration, property)
cargo test --all-targets --all-features

# Run specific integration test
cargo test --test sanity -- --nocapture drops_bounce

# Run property tests only
cargo test --test property_tests
```

### Benchmarking

```bash
cargo bench
```

### Linting & Formatting

```bash
# Check formatting
cargo fmt --check

# Apply formatting
cargo fmt

# Run clippy lints
cargo clippy --all-targets --all-features -- -D warnings
```

### xtasks

Common development tasks are available via a separate `xtask` crate. Run them using `cargo run --package xtask -- <command>`:

```bash
# Generate man page and shell completions (outputs to docs/)
cargo run --package xtask -- generate-docs

# Run checks (equivalent to cargo check)
cargo run --package xtask -- check
# Run tests (equivalent to cargo test)
cargo run --package xtask -- test

# Run clippy (equivalent to cargo clippy -- -D warnings)
cargo run --package xtask -- clippy

# Check formatting (equivalent to cargo fmt --check)
cargo run --package xtask -- fmt-check
```

### Nix Development Shell

If you have Nix installed with flakes enabled, use `nix develop` to enter a shell with all necessary development tools (Rust toolchain, `cargo-fuzz`, `cargo-audit`, `interception-tools`, `man`, etc.) and useful aliases (`ct` for test, `cl` for clippy, `cf` for fmt, `xt` for xtask).

### Fuzzing

Requires `cargo-fuzz`:

```bash
cargo install cargo-fuzz

# List fuzz targets
cargo fuzz list

# Run the stats fuzzer
cargo fuzz run fuzz_target_stats
```

## Contributing

Contributions are welcome! Please feel free to open an issue or submit a pull request on GitHub.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
