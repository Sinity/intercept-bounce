# intercept-bounce

`intercept-bounce` is an [Interception Tools](https://gitlab.com/interception/linux/tools) filter designed to eliminate keyboard chatter (also known as switch bounce). It reads Linux `input_event` structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output. Statistics are printed to stderr on exit.

This is particularly useful for mechanical keyboards which can sometimes register multiple presses or releases for a single physical key action due to noisy switch contacts.

## Features

* Filters keyboard chatter based on a configurable time threshold (`--debounce-time`).
* Integrates seamlessly with the Interception Tools ecosystem (reads from stdin, writes to stdout).
* Automatically collects and prints detailed statistics to stderr on exit (cleanly or via signal).
* Statistics include overall counts, per-key drop counts, bounce timings (min/avg/max), and near-miss timings.
* Optional periodic statistics dumping based on a time interval (`--log-interval`).
* Optional per-event logging for debugging (`--log-all-events`, `--log-bounces`).
* Optional verbose logging for internal state and thread activity (`--verbose`).
* Handles termination signals (`SIGINT`, `SIGTERM`, `SIGQUIT`) gracefully to ensure final statistics are reported.

## Prerequisites

* **Interception Tools:** Must be installed and configured. See the [Interception Tools documentation](https://gitlab.com/interception/linux/tools).
* **Build Environment:** Requires either a Rust toolchain or Nix with flakes enabled.

## Usage

`intercept-bounce` reads binary `input_event` data from `stdin` and writes the filtered binary data to `stdout`. It's designed to be placed in a pipeline between other Interception Tools like `intercept` (providing input) and `uinput` (consuming output).

## Options

> intercept-bounce [OPTIONS]
>
> *   `-t, --debounce-time <DURATION>`:
>    *   Sets the time threshold for bounce filtering (default: `25ms`). Accepts values like `10ms`, `0.5s`.
>    *   Events for the *same key code* and *same value* occurring faster than this threshold are discarded.
>    *   The "value" refers to the state of the key: `1` for press, `0` for release, `2` for repeat. Only press and release events are debounced.
> *   `--near-miss-threshold-time <DURATION>`:
>    *   Sets the time threshold for identifying "near-miss" events (default: `100ms`). Accepts values like `100ms`, `0.1s`.
>    *   Passed key events occurring within this time of the previous passed event for the same key/value are counted and reported in the statistics as near-misses. This helps identify keys that might be *almost* bouncing or have inconsistent timing just outside the debounce window.
>    *   Setting this to `0` effectively disables near-miss tracking.
>
> *   `--log-interval <DURATION>`:
>    *   Periodically dump statistics to stderr (default: `15m`). Accepts values like `60s`, `15m`, `1h`. Set to `0` to disable periodic dumps. Statistics are always printed on exit.
> *   `--log-all-events`:
>    *   Log details of *every* incoming event to stderr ([PASS] or [DROP]). Note: `EV_SYN` and `EV_MSC` events are skipped for cleaner output.
> * `--log-bounces`:
>   * Log details of *only dropped* (bounced) key events to stderr. This is ignored if `--log-all-events` is active.
> * `--stats-json`:
>   * Output statistics (on exit and periodic dumps) in JSON format to stderr instead of the default human-readable format.
> * `--verbose`:
>   * Enable verbose logging, including internal state, thread startup/shutdown messages, and detailed debug information.
> * `-h, --help`: Print help information.
> * `-V, --version`: Print version information.

### Examples

1.  **Basic Filtering (15ms window):**
    Pipe output from `intercept` (grabbing your keyboard) through `intercept-bounce` and into `uinput` to create a filtered virtual device. Replace `/dev/input/by-id/your-keyboard-event-device` with your actual device path.

    ```bash
    sudo sh -c 'intercept -g /dev/input/by-id/your-keyboard-event-device | intercept-bounce --debounce-time 15 | uinput -d /dev/input/by-id/your-keyboard-event-device'
    ```

    *(You'll likely need `sudo` or appropriate permissions for `intercept` and `uinput`)*.

2.  **Filtering with Bounce Logging:**
    Filter with a 20ms threshold and log only the events that get dropped. Detailed statistics will still print to stderr on exit.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 20 --log-bounces | uinput -d ...'
    ```

3.  **Debugging - Log All Events (No Filtering):**
    See every event passing through without filtering (`--debounce-time 0`), useful for observing raw input.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 0 --log-all-events | uinput -d ...'
    ```

4.  **Periodic Stats Dump:**
    Filter with the default 25ms threshold and print full stats to stderr every 60 seconds (in addition to the final stats on exit).

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --log-interval 60s | uinput -d ...'
    ```

5.  **Verbose Debugging:**
    Enable verbose internal logging. Combine with other logging flags for maximum detail.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --verbose --log-all-events | uinput -d ...'
    ```
    *(See "Mixed Output" note below regarding terminal echo)*.

6.  **udevmon Integration (YAML Example):**
    You can use `intercept-bounce` directly in your `udevmon.yaml` configuration for Interception Tools. Here are some example jobs:

    ```yaml
    # --- Example 1: Basic Filtering ---
    - JOB: "intercept -g $DEVNODE | intercept-bounce | uinput -d $DEVNODE"
      DEVICE:
        LINK: "/dev/input/by-id/usb-Your_Keyboard_Name-event-kbd" # Replace this!

    # --- Example 2: Filtering with Periodic Stats ---
    - JOB: "intercept -g $DEVNODE | intercept-bounce --debounce-time 15 --log-interval 300 | uinput -d $DEVNODE"
      DEVICE:
        LINK: "/dev/input/by-id/usb-Logitech_G915_WIRELESS_RGB_MECHANICAL_GAMING_KEYBOARD_*-event-kbd" # Replace/adjust this!

    # --- Example 3: Logging Only Bounced Events ---
    - JOB: "intercept -g $DEVNODE | intercept-bounce --debounce-time 20 --log-bounces | uinput -d ... # Replace ... with your device link"
      DEVICE:
        LINK: "/dev/input/by-id/usb-Another_Keyboard_*-event-kbd" # Replace this!
    ```

    *Find device links in `/dev/input/by-id/` or use `udevadm info` on your `/dev/input/eventX` device.*

    > More examples and up-to-date usage can be found in the README or at [https://github.com/sinity/intercept-bounce](https://github.com/sinity/intercept-bounce).

## How it Works

`intercept-bounce` employs a multi-threaded architecture optimized for low latency.

### Main Thread: Event Pipeline

1.  **Raw I/O:** Reads binary `input_event` data directly from the standard input file descriptor using raw `libc::read` calls, bypassing standard library buffering for minimal input latency.
2.  **Minimal State:** Maintains a small internal state tracking only the timestamp (in microseconds) of the last *passed* event for each unique combination of `(key_code, key_value)`. `key_value` is `1` for press, `0` for release, `2` for repeat.
3.  **Bounce Check:** When a new **key event** arrives (press or release only; repeats are ignored):
    *   Its timestamp is compared to the timestamp of the last *passed* event with the **exact same key code AND key value**.
    *   If the time difference is *less than* the configured `--debounce-time` threshold, the event is considered a chatter and is **dropped**.
    *   If the time difference is *greater than or equal to* the threshold, or if it's the first event seen for that specific `(key_code, key_value)` pair, the event is **passed**. Its timestamp is recorded as the new "last passed" time for that pair.
    *   Events with timestamps earlier than the last passed event (time going backwards) are always passed.
    *   A `--debounce-time` of `0` effectively disables filtering.
4.  **Non-Key Events:** Events that are not key events (e.g., `EV_SYN`, `EV_MSC`, `EV_REL`) are **always passed through** unmodified.
5.  **Raw Output:** Passed events are written directly to the standard output file descriptor using raw `libc::write` calls, minimizing output latency.
6.  **Logger Communication:** After processing an event, detailed information (the event itself, timestamps, bounce result) is sent *non-blockingly* over an internal channel to a separate Logger/Stats thread. If the logger thread falls behind and the channel buffer is full, log messages are dropped to prevent blocking the main event pipeline. A warning is printed to stderr *once* when dropping starts, and an info message is printed *once* when the logger catches up again.

### Logger/Stats Thread

1.  **Decoupled Processing:** Runs independently from the main event pipeline.
2.  **Receives Event Info:** Waits for `EventInfo` messages from the main thread via the channel.
3.  **Statistics Accumulation:** Updates detailed statistics (overall counts, per-key drop counts, bounce timings, near-miss timings) based on the received `EventInfo`. It maintains both cumulative stats for the entire run and interval stats for periodic reporting. Near-miss events are counted if they pass the filter and occur within the configured `--near-miss-threshold-time` of the previous passed event for the same key/value.
4.  **Event Logging:** If `--log-all-events` or `--log-bounces` is enabled, formats and prints the relevant event details to standard error. `EV_SYN` and `EV_MSC` events are skipped in `--log-all-events` mode for cleaner output.
5.  **Periodic Stats:** If `--log-interval` is set, this thread periodically calculates and prints the *interval* statistics (stats accumulated since the last dump) to standard error, then resets the interval counters.
6.  **Final Stats:** When the main thread signals shutdown (by closing the channel), the logger thread finishes processing any remaining messages and returns the final *cumulative* statistics object to the main thread.

### Statistics Output

Statistics provide insight into the filter's operation and are collected by the logger thread. **Final cumulative statistics are printed to stderr on exit** (clean EOF or signal termination).

*   **Status Header:** Shows the configured debounce and near-miss thresholds and the status of logging flags.
*   **Runtime:** Total duration from the first event seen to the last event seen by the main thread.
*   **Overall Statistics:** Total key events processed, passed, and dropped, along with the percentage dropped.
*   **Dropped Event Statistics:** A detailed breakdown for each key where events were dropped:
    *   Grouped by key code (e.g., `KEY_A (30)`).
    *   Shows drop counts for each state (`Press (1)`, `Release (0)`). *Note: Repeat (2) events are not debounced, so drop counts for repeats should always be zero.*
    *   Includes **Bounce Time** statistics (Min / Avg / Max) indicating the time difference between the dropped event and the previous *passed* event of the same type.
*   **Near-Miss Statistics:** Shows statistics for key events that were *passed* (not dropped) but occurred within the configured `--near-miss-threshold-time` of the previous *passed* event for that specific key code and value. Timings (Min / Avg / Max) relative to the previous event are shown.
*   **Periodic Logging (`--log-interval`):** If set > 0, the *interval* statistics block (stats accumulated since the last dump) is printed periodically by the logger thread.
*   **JSON Output (`--stats-json`):** Outputs final and periodic statistics in JSON format for easier machine parsing.

## Troubleshooting / Notes

### Why are key repeats not debounced?

Key repeat events (`value == 2`) are generated by the OS or keyboard firmware when you hold a key down, and are not the result of hardware bounce. Debouncing them would interfere with normal typing (e.g., holding "A" would not repeat as expected). Therefore, `intercept-bounce` **never debounces repeat events**—all repeats are always passed through, regardless of timing. Only press (`value == 1`) and release (`value == 0`) events are debounced.

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

---

## Additional Ideas

- **Configuration tips:**
  - How to choose a good debounce time for your keyboard.
  - How to test for chatter using `--log-all-events` and `--log-bounces`.
- **Performance:**
  - `intercept-bounce` is designed to be fast and low-overhead, suitable for real-time input pipelines. Benchmarks can be run using `cargo bench`.
- **Security:**
  - `intercept-bounce` does not require root itself, but you may need root to access input devices.
- **Platform support:**
  - Only tested on Linux with Interception Tools.
- **Contact:**
  - For bugs, feature requests, or questions, open an issue on [GitHub](https://github.com/sinity/intercept-bounce).
- **Systemd Service Unit:** Provide a ready-to-use `.service` file example.
- **Install Script:** A simple script to build and install (e.g., to `/usr/local/bin`).
- **Logging to File:** Instead of just stderr, allow logging directly to a specified file.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
