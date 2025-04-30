# intercept-bounce

`intercept-bounce` is an [Interception Tools](https://gitlab.com/interception/linux/tools) filter designed to eliminate keyboard chatter (also known as switch bounce). It reads Linux `input_event` structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output.

This is particularly useful for mechanical keyboards which can sometimes register multiple presses or releases for a single physical key action due to noisy switch contacts.

## Features Overview

* Filters keyboard chatter based on a configurable time threshold.
* Integrates seamlessly with the Interception Tools ecosystem.
* Provides detailed statistics on exit about filtered and passed events.
* Offers optional periodic statistics dumping and per-event logging for debugging.
* Handles termination signals gracefully to ensure statistics are reported.

## Installation

### Prerequisites

* **Interception Tools:** You need the Interception Tools installed and configured. See the [Interception Tools documentation](https://gitlab.com/interception/linux/tools).
* **Build Environment:** You need either a Rust toolchain or Nix with flakes enabled.

### Using Nix (Recommended)

If you have Nix installed with flakes enabled:

1. **Build:**

    ```bash
    # From the project directory
    nix build
    # The binary will be in ./result/bin/intercept-bounce
    ```

2. **Run Directly:**

    ```bash
    # From the project directory
    nix run . -- [OPTIONS]
    # Example: Run with default settings in a pipeline
    sudo sh -c 'intercept -g ... | nix run . -- | uinput -d ...'
    ```

### Using Cargo (Rust Toolchain)

1. **Install Rust:** Get it from [rustup.rs](https://rustup.rs/).
2. **Clone the repository:**

    ```bash
    git clone https://github.com/sinity/intercept-bounce.git
    cd intercept-bounce
    ```

3. **Build and install:**

```bash
    cargo install --path .
    ```

    The binary `intercept-bounce` will be installed in your Cargo bin directory (usually `~/.cargo/bin/`). Ensure this directory is in your `PATH`.

## Usage

`intercept-bounce` reads binary `input_event` data from `stdin` and writes the filtered binary data to `stdout`. It's designed to be placed in a pipeline between other Interception Tools like `intercept` (providing input) and `uinput` (consuming output).

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

1. **Basic Filtering (15ms window):**
    Pipe output from `intercept` (grabbing your keyboard) through `intercept-bounce` and into `uinput` to create a filtered virtual device. Replace `/dev/input/by-id/your-keyboard-event-device` with your actual device path.

    ```bash
    sudo sh -c 'intercept -g /dev/input/by-id/your-keyboard-event-device | intercept-bounce --debounce-time 15 | uinput -d /dev/input/by-id/your-keyboard-event-device'
    ```

    *(You'll likely need `sudo` or appropriate permissions for `intercept` and `uinput`)*.

2. **Filtering with Bounce Logging:**
    Filter with a 20ms threshold and log only the events that get dropped. Detailed statistics will still print to stderr on exit.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 20 --log-bounces | uinput -d ...'
    ```

3. **Debugging - Log All Events (No Filtering):**
    See every event passing through without filtering (`--debounce-time 0`), useful for observing raw input.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 0 --log-all-events | uinput -d ...'
    ```

4. **Periodic Stats Dump:**
    Filter with the default 10ms threshold and print full stats to stderr every 60 seconds (in addition to the final stats on exit).

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --log-interval 60 | uinput -d ...'
    ```

## How it Works

### Debouncing Logic

`intercept-bounce` filters events based on a simple time threshold. It works by remembering the timestamp of the last *passed* event for each unique combination of `(key_code, key_value)`. The `key_value` represents the key state: `1` for press, `0` for release, and `2` for repeat.

When a new **key event** arrives:

1. Its timestamp (in microseconds, derived from the `input_event`'s `timeval`) is compared to the timestamp of the last *passed* event with the **exact same key code AND key value**.
2. If the time difference is *less than* the configured `--debounce-time` threshold, the new event is considered a bounce/chatter and is **dropped** (not written to stdout).
3. If the time difference is *greater than or equal to* the threshold, or if it's the first event seen for that specific `(key_code, key_value)` pair, the event is **passed** (written to stdout). Its timestamp is then recorded as the new "last passed" time for that pair.
4. **Important:** Filtering only applies to events with the *same code and value*. A rapid key press followed immediately by a release will *not* be filtered, as their `key_value` differs (1 vs 0).
5. Events where the timestamp appears to go backwards compared to the last recorded event are *not* treated as bounces and are always passed.
6. A `--debounce-time` of `0` effectively disables all filtering.

### Non-Key Events

Events that are not key events (e.g., `EV_SYN`, `EV_MSC`, `EV_REL`, `EV_ABS`, `EV_LED`) are **always passed through** unmodified, as they are not relevant to key bounce.

### Statistics and Logging

Statistics provide insight into the filter's operation and are **always collected and printed to stderr on exit** (either clean EOF or signal termination via `SIGINT`/`SIGTERM`/`SIGQUIT`).

*   **Status Header:** Shows the configured debounce threshold and the status of logging flags.
*   **Overall Statistics:** Total key events processed, passed, and dropped, along with the percentage dropped.
*   **Dropped Event Statistics:** A detailed breakdown for each key where events were dropped:
    *   Grouped by key code (e.g., `KEY_A (30)`).
    *   Shows drop counts for each state (`Press (1)`, `Release (0)`, `Repeat (2)`).
    *   Includes **Bounce Time** statistics (Min / Avg / Max) indicating the time difference between the dropped event and the previous *passed* event of the same type. This helps understand the timing of the chatter being filtered.
*   **Near-Miss Statistics:** Shows statistics for key events that were *passed* (not dropped) but occurred within 100ms of the previous event for that specific key code and value. This can help identify keys that are close to the debounce threshold or exhibit borderline chatter. Timings (Min / Avg / Max) relative to the previous event are shown.
*   **Periodic Logging (`--log-interval`):** If set > 0, the full statistics block is also printed periodically during runtime.
*   **Event Logging (`--log-all-events`, `--log-bounces`):** Provides per-event details logged to stderr *after* the filtering decision, useful for fine-grained debugging.

## Troubleshooting / Notes

### Mixed Output in Terminal (Logging + Typed Characters)

When running `intercept-bounce` interactively in a pipeline (e.g., `intercept | intercept-bounce | uinput`) and using logging flags (`--log-all-events` or `--log-bounces`), you might see the characters you type mixed in with the log output printed to stderr.

This happens because:

1. `intercept` captures the raw key events.
2. `intercept-bounce` logs these events to stderr.
3. Your terminal (TTY) *also* echoes the characters you type to the screen by default.

These two outputs (stderr logging and terminal echo) are independent and can get interleaved on your display.

**Solution:** Temporarily disable terminal echo while running the command pipeline:

```bash
stty -echo && sudo sh -c 'intercept -g ... | intercept-bounce [OPTIONS] | uinput -d ...' ; stty echo
```

* `stty -echo`: Disables terminal echo.
* `... your command ...`: Run the pipeline.
* `stty echo`: **Crucially**, re-enables terminal echo afterwards.

This will prevent your typed characters from appearing amidst the log output.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
