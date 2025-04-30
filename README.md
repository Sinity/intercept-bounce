# intercept-bounce

[![Crates.io](https://img.shields.io/crates/v/intercept-bounce.svg)](https://crates.io/crates/intercept-bounce)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://github.com/sinity/intercept-bounce/actions/workflows/rust.yml/badge.svg)](https://github.com/sinity/intercept-bounce/actions/workflows/rust.yml)

`intercept-bounce` is an [Interception Tools](https://gitlab.com/interception/linux/tools) filter designed to eliminate keyboard chatter (also known as switch bounce). It reads Linux `input_event` structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output.

This is particularly useful for mechanical keyboards which can sometimes register multiple presses or releases for a single physical key action due to noisy switch contacts.

## Features

*   **Configurable Debounce Threshold:** Set the time threshold (in milliseconds) below which duplicate key events (same key code and value) are discarded.
*   **Statistics:** Automatically view detailed statistics on exit (processed, passed, dropped counts; per-key drop counts; bounce timings; near-miss timings).
*   **Event Logging:** Optionally log all incoming events or only the dropped (bounced) events for debugging.
*   **Periodic Stats:** Optionally dump statistics periodically based on a time interval.
*   **Integration with Interception Tools:** Designed to be easily plugged into an interception chain using tools like `intercept` and `uinput`.

## Installation

### Prerequisites

*   **Rust:** Ensure you have a recent Rust toolchain installed. You can get it from [rustup.rs](https://rustup.rs/).
*   **Interception Tools:** You need the Interception Tools installed and configured on your system. See the [Interception Tools documentation](https://gitlab.com/interception/linux/tools) for installation instructions.

### From Crates.io

```bash
cargo install intercept-bounce
```

### From Source

1.  Clone the repository:
    ```bash
    git clone https://github.com/sinity/intercept-bounce.git
    cd intercept-bounce
    ```
2.  Build and install:
    ```bash
    cargo install --path .
    ```
    The binary `intercept-bounce` will be installed in your Cargo bin directory (usually `~/.cargo/bin/`).

## Usage

`intercept-bounce` reads `input_event` data from `stdin` and writes filtered data to `stdout`. It's typically used in a pipeline with Interception Tools.

```
intercept-bounce [OPTIONS]
```

### Options

*   `-t, --debounce-time <MS>`:
    *   Sets the time threshold for bounce filtering in milliseconds (default: `10`).
    *   Events for the *same key code* and *same value* (press/release/repeat) occurring faster than this threshold are dropped.
    *   Setting `--debounce-time 0` effectively disables filtering.
*   `--log-interval <SECONDS>`:
    *   Periodically dump statistics to stderr every `SECONDS` seconds (default: `0` = disabled). Statistics are always printed on exit.
*   `--log-all-events`:
    *   Log details of *every* incoming event to stderr, prefixed with `[PASS]` or `[DROP]`. Includes non-key events.
*   `--log-bounces`:
    *   Log details of *only dropped* (bounced) key events to stderr. This is ignored if `--log-all-events` is active.
*   `-h, --help`: Print help information.
*   `-V, --version`: Print version information.

### Examples

1.  **Basic Filtering (15ms window):**
    Pipe output from `intercept` (grabbing your keyboard) through `intercept-bounce` and into `uinput` to create a filtered virtual device. Replace `/dev/input/by-id/your-keyboard-event-device` with your actual device path.

    ```bash
    sudo sh -c 'intercept -g /dev/input/by-id/your-keyboard-event-device | intercept-bounce --debounce-time 15 | uinput -d /dev/input/by-id/your-keyboard-event-device'
    ```
    *(You'll likely need `sudo` or appropriate permissions for `intercept` and `uinput`)*.

2.  **Filtering with Bounce Logging:**
    Filter with a 20ms threshold and log only the events that get dropped. Detailed statistics will still print on exit.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 20 --log-bounces | uinput -d ...'
    ```

3.  **Debugging - Log All Events (No Filtering):**
    See every event passing through without filtering (`--debounce-time 0`), useful for observing raw input.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 0 --log-all-events | uinput -d ...'
    ```

4.  **Periodic Stats Dump:**
    Filter with the default 10ms threshold and print full stats to stderr every 60 seconds.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --log-interval 60 | uinput -d ...'
    ```

## How it Works

`intercept-bounce` maintains a timestamp of the last *passed* event for each unique combination of key code and key value (press=1, release=0, repeat=2).

When a new key event arrives:
1.  It checks if an event with the *same key code* and *same value* (press=1, release=0, repeat=2) has passed within the configured `--debounce-time`.
2.  If yes (time difference < threshold), the new event is considered a bounce and is **dropped** (not written to stdout).
3.  If no (time difference >= threshold, or it's the first event for that key/value), the event is **passed** (written to stdout), and its timestamp is recorded as the new "last passed" time for that specific key/value combination.
4.  Non-key events (like `EV_SYN` or `EV_MSC`) are always passed through unchanged.
5.  Statistics (including detailed timings for dropped events and near-miss passed events < 100ms) are collected during this process and printed to stderr upon termination (or periodically if requested via `--log-interval`).

## License

Licensed under either of

*   Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
*   MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
