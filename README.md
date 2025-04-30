# intercept-bounce

Small, single-purpose filter for [Interception Tools](https://gitlab.com/interception/linux/tools).  
Filters key repeat noise from faulty or noisy keyboards.

This is an independent user-mode filter, intended for use with `udevmon`.

## Features

- Removes rapid duplicate key events (chatter/bounce) based on key code *and* state (press/release/repeat).
- Configurable time window (milliseconds) for bounce detection (default: 10ms).
- Passes non-key events (like `EV_SYN`) through unmodified.
- Composable in standard Interception Tools pipelines.

## Status

Work in progress. Use at your own risk.

## License

MIT OR Apache-2.0
