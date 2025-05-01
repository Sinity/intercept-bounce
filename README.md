# intercept-bounce

`intercept-bounce` is an [Interception Tools](https://gitlab.com/interception/linux/tools) filter designed to eliminate keyboard chatter (also known as switch bounce). It reads Linux `input_event` structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output.

This is particularly useful for mechanical keyboards which can sometimes register multiple presses or releases for a single physical key action due to noisy switch contacts.

## Features

* Filters keyboard chatter based on a configurable time threshold (`--debounce-time`).
* Integrates seamlessly with the Interception Tools ecosystem (reads from stdin, writes to stdout).
* Automatically collects and prints detailed statistics to stderr on exit (cleanly or via signal).
* Statistics include overall counts, per-key drop counts, bounce timings (min/avg/max), and near-miss timings.
* Optional periodic statistics dumping based on a time interval (`--log-interval`).
* Optional per-event logging for debugging (`--log-all-events`, `--log-bounces`).
* Handles termination signals (`SIGINT`, `SIGTERM`, `SIGQUIT`) gracefully to ensure final statistics are reported.

## Prerequisites

* **Interception Tools:** Must be installed and configured. See the [Interception Tools documentation](https://gitlab.com/interception/linux/tools).
* **Build Environment:** Requires either a Rust toolchain or Nix with flakes enabled.

## Usage

`intercept-bounce` reads binary `input_event` data from `stdin` and writes the filtered binary data to `stdout`. It's designed to be placed in a pipeline between other Interception Tools like `intercept` (providing input) and `uinput` (consuming output).

## Options

> intercept-bounce [OPTIONS]
>
> * `-t, --debounce-time <MS>`:
>   * Sets the time threshold for bounce filtering in milliseconds (default: `10`).
>   * Events for the *same key code* and *same value* (press/release/repeat) occurring faster than this threshold are dropped.
>   * Setting `--debounce-time 0` effectively disables filtering.
> * `--log-interval <SECONDS>`:
>   * Periodically dump statistics to stderr every `SECONDS` seconds (default: `0` = disabled). Statistics are always printed on exit.
> * `--log-all-events`:
>   * Log details of *every* incoming event to stderr, prefixed with `[PASS]` or `[DROP]`. Includes non-key events.
> * `--log-bounces`:
>   * Log details of *only dropped* (bounced) key events to stderr. This is ignored if `--log-all-events` is active.
> * `-h, --help`: Print help information.
> * `-V, --version`: Print version information.

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

5. **udevmon Integration (YAML Example):**
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
    - JOB: "intercept -g $DEVNODE | intercept-bounce --debounce-time 20 --log-bounces | uinput -d $DEVNODE"
      DEVICE:
        LINK: "/dev/input/by-id/usb-Another_Keyboard_*-event-kbd" # Replace this!
    ```

    *Find device links in `/dev/input/by-id/` or use `udevadm info` on your `/dev/input/eventX` device.*

    > More examples and up-to-date usage can be found in the README or at [https://github.com/sinity/intercept-bounce](https://github.com/sinity/intercept-bounce).

## How it Works

### Debouncing Logic

`intercept-bounce` filters events based on a simple time threshold. It works by remembering the timestamp of the last *passed* event for each unique combination of `(key_code, key_value)`. The `key_value` represents the key state: `1` for press, `0` for release, and `2` for repeat.

When a new **key event** arrives:

1. Its timestamp (in microseconds, derived from the `input_event`'s `timeval`) is compared to the timestamp of the last *passed* event with the **exact same key code AND key value**.
2. If the time difference is *less than* the configured `--debounce-time` threshold, the new event is considered a bounce/chatter and is **dropped** (not written to stdout).
3. If the time difference is *greater than or equal to* the threshold, or if it's the first event seen for that specific `(key_code, key_value)` pair, the event is **passed** (written to stdout). Its timestamp is then recorded as the new "last passed" time for that pair.
4. **Important:** Filtering only applies to events with the *same code and value*. A rapid key press followed immediately by a release will *not* be filtered, as their `key_value` differs (1 vs 0).
5. **Key repeats (`value == 2`) are *not* debounced.** All repeat events are always passed through, regardless of timing. This matches user expectations and standard debounce tool behavior, since repeats are generated by the OS or firmware, not by hardware bounce.
6. Events where the timestamp appears to go backwards compared to the last recorded event are *not* treated as bounces and are always passed.
7. A `--debounce-time` of `0` effectively disables all filtering.

### Non-Key Events

Events that are not key events (e.g., `EV_SYN`, `EV_MSC`, `EV_REL`, `EV_ABS`, `EV_LED`) are **always passed through** unmodified, as they are not relevant to key bounce.

### Statistics and Logging

Statistics provide insight into the filter's operation and are **always collected and printed to stderr on exit** (either clean EOF or signal termination via `SIGINT`/`SIGTERM`/`SIGQUIT`).

* **Status Header:** Shows the configured debounce threshold and the status of logging flags.
* **Overall Statistics:** Total key events processed, passed, and dropped, along with the percentage dropped.
* **Dropped Event Statistics:** A detailed breakdown for each key where events were dropped:
  * Grouped by key code (e.g., `KEY_A (30)`).
  * Shows drop counts for each state (`Press (1)`, `Release (0)`, `Repeat (2)`).  
    *Note: By default, repeat events are not debounced, so drop counts for repeats will always be zero unless you modify the code.*
  * Includes **Bounce Time** statistics (Min / Avg / Max) indicating the time difference between the dropped event and the previous *passed* event of the same type. This helps understand the timing of the chatter being filtered.
* **Near-Miss Statistics:** Shows statistics for key events that were *passed* (not dropped) but occurred within 100ms of the previous event for that specific key code and value. This can help identify keys that are close to the debounce threshold or exhibit borderline chatter. Timings (Min / Avg / Max) relative to the previous event are shown.
* **Periodic Logging (`--log-interval`):** If set > 0, the full statistics block is also printed periodically during runtime, including both cumulative and interval (since last dump) stats.
* **Event Logging (`--log-all-events`, `--log-bounces`):** Provides per-event details logged to stderr *after* the filtering decision, useful for fine-grained debugging.

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

## Additional Ideas for README

- **Configuration tips:**  
  - How to choose a good debounce time for your keyboard.
  - How to test for chatter using `--log-all-events` and `--log-bounces`.
- **Performance:**  
  - `intercept-bounce` is designed to be fast and low-overhead, suitable for real-time input pipelines.
- **Extending the tool:**  
  - How to contribute new features (e.g., per-key debounce times, configuration files).
- **Security:**  
  - `intercept-bounce` does not require root itself, but you may need root to access input devices.
- **Platform support:**  
  - Only tested on Linux with Interception Tools.
- **Contact:**  
  - For bugs, feature requests, or questions, open an issue on [GitHub](https://github.com/sinity/intercept-bounce).

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
