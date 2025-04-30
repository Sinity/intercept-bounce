# intercept-bounce

Small, single-purpose filter for [Interception Tools](https://gitlab.com/interception/linux/tools).  
Filters key repeat noise from faulty or noisy keyboards.

This is an independent user-mode filter, intended for use with `udevmon`.

## Features

- Removes rapid duplicate key events (chatter/bounce) based on key code *and* state (press/release/repeat).
- Configurable time window (`--window`, default: 10ms). Events occurring faster than this window are filtered. A higher value filters more aggressively.
- Passes non-key events (like `EV_SYN`) through unmodified.
- Composable in standard Interception Tools pipelines.
- **Verbose Mode (`--verbose`, `-v`):**
    - Enables statistics collection (total processed, total dropped, per-key drops, basic timing).
    - Prints statistics to `stderr` on clean exit (EOF) or when receiving `SIGINT`/`SIGTERM`/`SIGQUIT`.
    - Enables periodic statistics dumping using `--log-interval`.
    - Enables extended logging of key repeats (value=2) to `stderr` if they occur within `max(window, 100ms)` of the previous event for that key, even if not dropped.
- **Periodic Logging (`--log-interval N`):** In verbose mode, dumps statistics to `stderr` every `N` key events processed (default 0 = disabled).

## Status

Work in progress. Use at your own risk.

## License

MIT OR Apache-2.0
