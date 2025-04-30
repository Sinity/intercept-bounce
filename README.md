# intercept-bounce

[![Crates.io](https://img.shields.io/crates/v/intercept-bounce.svg)](https://crates.io/crates/intercept-bounce)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://github.com/sinity/intercept-bounce/actions/workflows/rust.yml/badge.svg)](https://github.com/sinity/intercept-bounce/actions/workflows/rust.yml)

`intercept-bounce` is a command-line filter designed for use with [Interception Tools](https://gitlab.com/interception/linux/tools). It reads Linux `input_event` structs from standard input, filters out rapid duplicate key events (commonly known as key chatter or switch bounce), and writes the filtered events to standard output.

This is useful for keyboards (especially mechanical ones) that sometimes register multiple presses or releases for a single physical key action.

## Features

*   **Configurable Bounce Window:** Set the time window (in milliseconds) below which duplicate key events (same key code and value) are discarded.
*   **Statistics:** View detailed statistics on processed, passed, and dropped key events, including per-key drop counts and bounce timing information.
*   **Event Logging:** Optionally log all incoming events or only the dropped (bounced) events for debugging.
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

*   `-w, --window <MILLISECONDS>`:
    *   Sets the time window for bounce filtering (default: `10`).
    *   Events for the *same key code* and *same value* (press/release/repeat) occurring faster than this window are dropped.
    *   Setting `--window 0` effectively disables filtering, passing all events through.
*   `-s, --stats`:
    *   Collect detailed statistics, including per-key bounce timing (min/avg/max).
    *   Enables periodic logging if `--log-interval` is set.
    *   Statistics are always printed on exit (cleanly or via signal), but timing details require this flag.
*   `--log-interval <N>`:
    *   If `--stats` is enabled and `N` > 0, dump statistics to stderr every `N` key events processed (default: `0` = disabled).
*   `--log-all-events`:
    *   Log details of *every* incoming event to stderr, prefixed with `[PASS]` or `[DROP]`.
*   `--log-bounces`:
    *   Log details of *only dropped* (bounced) key events to stderr. This is ignored if `--log-all-events` is active.
*   `-h, --help`: Print help information.
*   `-V, --version`: Print version information.

### Examples

1.  **Basic Filtering (15ms window):**
    Pipe output from `intercept` (grabbing your keyboard) through `intercept-bounce` and into `uinput` to create a filtered virtual device. Replace `/dev/input/by-id/your-keyboard-event-device` with your actual device.

    ```bash
    sudo sh -c 'intercept -g /dev/input/by-id/your-keyboard-event-device | intercept-bounce --window 15 | uinput -d /dev/input/by-id/your-keyboard-event-device'
    ```
    *(You'll likely need `sudo` or appropriate permissions for `intercept` and `uinput`)*.

2.  **Filtering with Stats and Bounce Logging:**
    Filter with a 20ms window, collect detailed stats, and log only the events that get dropped.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --window 20 --stats --log-bounces | uinput -d ...'
    ```
    *(Stats will print to stderr when the command exits)*.

3.  **Debugging - Log All Events (No Filtering):**
    See every event passing through without filtering, useful for observing raw input.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --window 0 --log-all-events | uinput -d ...'
    ```

4.  **Periodic Stats Dump:**
    Filter with a 10ms window and print full stats to stderr every 1000 key events processed.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --stats --log-interval 1000 | uinput -d ...'
    ```

## How it Works

`intercept-bounce` maintains a timestamp of the last *passed* event for each unique combination of key code and key value (press=1, release=0, repeat=2).

When a new key event arrives:
1.  It checks if an event with the *same key code* and *same value* has passed within the configured `--window`.
2.  If yes, the new event is considered a bounce and is **dropped** (not written to stdout).
3.  If no (either it's the first event for that key/value, or the time difference is >= window), the event is **passed** (written to stdout), and its timestamp is recorded as the new "last passed" time for that specific key/value combination.
4.  Non-key events (like `EV_SYN` or `EV_MSC`) are always passed through unchanged.
5.  Statistics are collected during this process and printed to stderr upon termination (or periodically if requested).

## License

Licensed under either of

*   Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
*   MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
