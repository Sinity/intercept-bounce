complete -c intercept-bounce -s t -l debounce-time -d 'Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default: 25ms). The "value" refers to the state of the key: `1` for press, `0` for release, `2` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"' -r
complete -c intercept-bounce -l near-miss-threshold-time -d 'Threshold for logging "near-miss" events. Passed key events occurring within this time of the previous passed event are logged/counted. (Default: 100ms) Accepts values like "100ms", "0.1s"' -r
complete -c intercept-bounce -l log-interval -d 'Periodically dump statistics to stderr. (Default: 15m). Set to "0" to disable periodic dumps. Accepts values like "60s", "15m", "1h"' -r
complete -c intercept-bounce -l ring-buffer-size -d 'Size of the ring buffer for storing recently passed events (for debugging). Set to 0 to disable. (Default: 0)' -r
complete -c intercept-bounce -l debounce-key -d 'Key codes or names to debounce. When present, only these keys are debounced (all others pass through). Takes precedence over `--ignore-key`. Example: `--debounce-key KEY_ENTER` (repeat flag for multiple keys)' -r
complete -c intercept-bounce -l ignore-key -d 'Key codes or names to ignore (never debounce) unless they also appear in `--debounce-key`. Example: `--ignore-key 114` or `--ignore-key KEY_VOLUMEDOWN`' -r
complete -c intercept-bounce -l otel-endpoint -d 'OTLP endpoint URL for exporting traces and metrics (e.g., "http://localhost:4317")' -r
complete -c intercept-bounce -l log-all-events -d 'Log details of *every* incoming event to stderr ([PASS] or [DROP])'
complete -c intercept-bounce -l log-bounces -d 'Log details of *only dropped* (bounced) key events to stderr'
complete -c intercept-bounce -l list-devices -d 'List available input devices and their capabilities (requires root)'
complete -c intercept-bounce -l stats-json -d 'Output statistics as JSON format to stderr on exit and periodic dump'
complete -c intercept-bounce -l verbose -d 'Enable verbose logging (internal state, thread startup, etc)'
complete -c intercept-bounce -s h -l help -d 'Print help (see more with \'--help\')'
complete -c intercept-bounce -s V -l version -d 'Print version'
