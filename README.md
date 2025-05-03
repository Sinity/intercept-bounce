# intercept-bounce

`intercept-bounce` is an [Interception Tools](https://gitlab.com/interception/linux/tools) filter designed to eliminate keyboard chatter (also known as switch bounce). It reads Linux `input_event` structs from standard input, filters out rapid duplicate key events below a configurable time threshold, and writes the filtered events to standard output. Statistics are printed to stderr on exit.

This is particularly useful for mechanical keyboards which can sometimes register multiple presses or releases for a single physical key action due to noisy switch contacts.

## Features

* Filters keyboard chatter based on a configurable time threshold (`--debounce-time`).
* Tracks and reports "near-miss" events that occur just outside the debounce window (`--near-miss-threshold-time`).
* Integrates seamlessly with the Interception Tools ecosystem (reads from stdin, writes to stdout).
* Automatically collects and prints detailed statistics to stderr on exit (cleanly or via signal).
* Statistics include overall counts, per-key drop counts, bounce timings (min/avg/max), and near-miss timings.
* Optional periodic statistics dumping based on a time interval (`--log-interval`).
* Optional per-event logging for debugging (`--log-all-events`, `--log-bounces`).
* Optional verbose logging for internal state and thread activity (`--verbose`).
* Handles termination signals (`SIGINT`, `SIGTERM`, `SIGQUIT`) gracefully to ensure final statistics are reported.
* Includes unit tests, integration tests (including high-throughput simulation), property tests, and fuzzing for robustness.
* Supports benchmarking different internal queue implementations via Cargo features.

## Prerequisites

*   **Interception Tools:** Must be installed and configured. See the [Interception Tools documentation](https://gitlab.gitlab.io/interception/linux/tools).
*   **Build Environment:** Requires either a Rust toolchain or Nix with flakes enabled.

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
>    *   Setting this to `0ms` effectively disables near-miss tracking.
>
> *   `--log-interval <DURATION>`:
>    *   Periodically dump statistics to stderr (default: `15m`). Accepts values like `60s`, `15m`, `1h`. Set to `0s` to disable periodic dumps. Statistics are always printed on exit.
> *   `--log-all-events`:
>    *   Log details of *every* incoming event to stderr ([PASS] or [DROP]). Note: `EV_SYN` and `EV_MSC` events are skipped for cleaner output.
> * `--log-bounces`:
>   * Log details of *only dropped* (bounced) key events to stderr. This is ignored if `--log-all-events` is active.
> * `--list-devices`:
>   * List available input devices and their capabilities (requires read access to `/dev/input/event*`, typically root).
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
    sudo sh -c 'intercept -g /dev/input/by-id/your-keyboard-event-device | intercept-bounce --debounce-time 15ms | uinput -d /dev/input/by-id/your-keyboard-event-device'
    ```

    *(You'll likely need `sudo` or appropriate permissions for `intercept` and `uinput`)*.

2.  **Filtering with Bounce Logging:**
    Filter with a 20ms threshold and log only the events that get dropped. Detailed statistics will still print to stderr on exit.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 20ms --log-bounces | uinput -d ...'
    ```

3.  **Debugging - Log All Events (No Filtering):**
    See every event passing through without filtering (`--debounce-time 0ms`), useful for observing raw input.

    ```bash
    sudo sh -c 'intercept -g ... | intercept-bounce --debounce-time 0ms --log-all-events | uinput -d ...'
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
    - JOB: "intercept -g $DEVNODE | intercept-bounce --debounce-time 15ms --log-interval 300s | uinput -d $DEVNODE"
      DEVICE:
        LINK: "/dev/input/by-id/usb-Logitech_G915_WIRELESS_RGB_MECHANICAL_GAMING_KEYBOARD_*-event-kbd" # Replace/adjust this!

    # --- Example 3: Logging Only Bounced Events ---
    - JOB: "intercept -g $DEVNODE | intercept-bounce --debounce-time 20ms --log-bounces | uinput -d ... # Replace ... with your device link"
      DEVICE:
        LINK: "/dev/input/by-id/usb-Another_Keyboard_*-event-kbd" # Replace this!
    ```

    *Find device links in `/dev/input/by-id/` or use `udevadm info` on your `/dev/input/eventX` device.*

    > More examples and up-to-date usage can be found in the README or at [https://github.com/sinity/intercept-bounce](https://github.com/sinity/intercept-bounce).

## Testing

The project includes various testing methods to ensure correctness and robustness.

*   **Unit Tests:** Located in `tests/unit_*.rs`, these test individual modules and logic components in isolation. Run with `cargo test`.
*   **Integration Tests:** Located in `tests/sanity.rs`, these test the main binary pipeline by piping simulated input events and checking the output. This includes tests for basic filtering, edge cases, complex sequences, and a high-throughput simulation (`test_high_throughput`). Run with `cargo test`.
*   **Property Tests:** Located in `tests/property_tests.rs`, these use `proptest` to generate a wide range of inputs and verify that the filter behaves according to defined properties (e.g., output events are a subset of input, timestamps are non-decreasing for passed events). Run with `cargo test`.
*   **Fuzzing:** Located in `fuzz/fuzz_targets/`, this uses `libfuzzer-sys` to test the filter with malformed or unexpected raw input event data. This helps discover crashes or panics caused by invalid inputs. Fuzz targets include testing the core filter logic (`fuzz_target_1`), the statistics accumulation (`fuzz_target_stats`), and the statistics printing/formatting (`fuzz_target_stats_print`). Requires a nightly Rust toolchain and specific build commands. See the fuzzing documentation for details on building and running fuzz targets.

To run all tests (excluding fuzzing, which requires a separate setup):

```bash
cargo test
```

To run benchmarks:

```bash
cargo bench
```

### Benchmarking Channel Implementations

By default, `intercept-bounce` uses `crossbeam-channel::bounded` for communication between the main processing thread and the logger thread. You can benchmark an alternative lock-free queue implementation, `crossbeam-queue::ArrayQueue`, using a Cargo feature.

To run benchmarks using `crossbeam-queue::ArrayQueue`:

```bash
cargo bench --features use_lockfree_queue
```

Compare the results, particularly for the `logger::channel_send_burst` benchmark, to see the performance characteristics of each implementation under load.

### Generating Shell Completions and Man Pages

You can generate shell completion scripts (Bash, Zsh, Fish, etc.) and a man page using the provided helper binary:

```bash
# Ensure OUT_DIR is set (or default to target/generated)
export OUT_DIR=target/generated

# Run the generation binary
cargo run --bin generate-cli-files
```
The generated files will be placed in the `target/generated` directory (or the directory specified by `OUT_DIR`). You can then install them to the appropriate locations on your system (e.g., `/usr/local/share/man/man1/` for the man page, `/usr/share/bash-completion/completions/` for Bash completions).
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
