# intercept-bounce

[![Crates.io](https://img.shields.io/crates/v/intercept-bounce.svg)](https://crates.io/crates/intercept-bounce)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://github.com/sinity/intercept-bounce/actions/workflows/rust.yml/badge.svg)](https://github.com/sinity/intercept-bounce/actions/workflows/rust.yml)

`intercept-bounce` is an [Interception Tools](https://gitlab.com/interception/linux/tools) filter designed to eliminate keyboard chatter (also known as switch bounce). It reads Linux `input_event` structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output.

This is particularly useful for mechanical keyboards which can sometimes register multiple presses or releases for a single physical key action due to noisy switch contacts.

## Features

*   **Configurable Debounce Threshold:** Set the time threshold (in milliseconds) below which duplicate key events (same key code *and* value) are discarded.
*   **Automatic Statistics:** Automatically collects and prints detailed statistics to stderr on exit (cleanly or via signal). Includes overall counts, per-key drop counts, bounce timings (min/avg/max), and near-miss timings (passed events < 100ms).
*   **Event Logging:** Optionally log all incoming events (`--log-all-events`) or only the dropped events (`--log-bounces`) to stderr for debugging, showing `[PASS]` or `[DROP]` status.
*   **Periodic Stats:** Optionally dump statistics periodically based on a time interval (`--log-interval`).
*   **Signal Handling:** Gracefully handles `SIGINT`, `SIGTERM`, and `SIGQUIT` by printing final statistics before exiting.
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

### Debouncing Logic

`intercept-bounce` filters events based on a simple time threshold. It works by remembering the timestamp of the last *passed* event for each unique combination of `(key_code, key_value)`. The `key_value` represents the key state: `1` for press, `0` for release, and `2` for repeat.

When a new **key event** arrives:

1.  Its timestamp (in microseconds, derived from the `input_event`'s `timeval`) is compared to the timestamp of the last *passed* event with the **exact same key code AND key value**.
2.  If the time difference is *less than* the configured `--debounce-time` threshold, the new event is considered a bounce/chatter and is **dropped** (not written to stdout).
3.  If the time difference is *greater than or equal to* the threshold, or if it's the first event seen for that specific `(key_code, key_value)` pair, the event is **passed** (written to stdout). Its timestamp is then recorded as the new "last passed" time for that pair.
4.  **Important:** Filtering only applies to events with the *same code and value*. A rapid key press followed immediately by a release will *not* be filtered, as their `key_value` differs (1 vs 0).
5.  Events where the timestamp appears to go backwards compared to the last recorded event are *not* treated as bounces and are always passed.
6.  A `--debounce-time` of `0` effectively disables all filtering.

### Non-Key Events

Events that are not key events (e.g., `EV_SYN`, `EV_MSC`, `EV_REL`, `EV_ABS`, `EV_LED`) are **always passed through** unmodified, as they are not relevant to key bounce.

### Statistics and Logging

*   **Collection:** Statistics are always collected internally while the filter runs.
*   **Output:** Statistics are automatically printed to `stderr` when the process exits, either cleanly (input stream ends) or due to receiving `SIGINT`, `SIGTERM`, or `SIGQUIT`. A separate thread handles signal catching to ensure stats are printed reliably.
*   **Content:**
    *   *Overall:* Total key events processed, passed, and dropped, plus the percentage dropped.
    *   *Dropped Events:* Detailed breakdown per key, showing drop counts for press, release, and repeat states. Includes minimum, average, and maximum time differences (relative to the previous passed event of the same type) that caused an event to be dropped (bounce time).
    *   *Near-Miss Events:* Statistics for key events that *passed* but occurred within 100ms of the previous event for that key/value pair. This helps identify potential bounce activity just outside the configured threshold. Timings (min/avg/max) are shown relative to the previous event.
    *   *Formatting:* Timestamps and time differences in the stats output are formatted for readability (e.g., `12.3 ms`, `500 µs`).
*   **Periodic Logging:** If `--log-interval <SECONDS>` is set to a value greater than 0, the full statistics block will also be printed to stderr periodically, approximately every specified number of seconds (triggered by the next event arriving after the interval has passed).
*   **Event Logging:** The `--log-all-events` and `--log-bounces` flags provide verbose, per-event logging to stderr, indicating whether each event was `[PASS]`ed or `[DROP]`ped and showing relevant timing information. This logging occurs *after* the filtering decision has been made for the event.

## Troubleshooting / Notes

### Mixed Output in Terminal (Logging + Typed Characters)

When running `intercept-bounce` interactively in a pipeline (e.g., `intercept | intercept-bounce | uinput`) and using logging flags (`--log-all-events` or `--log-bounces`), you might see the characters you type mixed in with the log output printed to stderr.

This happens because:
1.  `intercept` captures the raw key events.
2.  `intercept-bounce` logs these events to stderr.
3.  Your terminal (TTY) *also* echoes the characters you type to the screen by default.

These two outputs (stderr logging and terminal echo) are independent and can get interleaved on your display.

**Solution:** Temporarily disable terminal echo while running the command pipeline:

```bash
stty -echo && sudo sh -c 'intercept -g ... | intercept-bounce [OPTIONS] | uinput -d ...' ; stty echo
```
*   `stty -echo`: Disables terminal echo.
*   `... your command ...`: Run the pipeline.
*   `stty echo`: **Crucially**, re-enables terminal echo afterwards.

This will prevent your typed characters from appearing amidst the log output.

## License

Licensed under either of

*   Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
*   MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
