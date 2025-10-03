module completions {

  # An Interception Tools filter to eliminate keyboard chatter (switch bounce).
  export extern intercept-bounce [
    --debounce-time(-t): string # Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default: 25ms). The "value" refers to the state of the key: `1` for press, `0` for release, `2` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"
    --near-miss-threshold-time: string # Threshold for logging "near-miss" events. Passed key events occurring within this time of the previous passed event are logged/counted. (Default: 100ms) Accepts values like "100ms", "0.1s"
    --log-interval: string    # Periodically dump statistics to stderr. (Default: 15m). Set to "0" to disable periodic dumps. Accepts values like "60s", "15m", "1h"
    --log-all-events          # Log details of *every* incoming event to stderr ([PASS] or [DROP])
    --log-bounces             # Log details of *only dropped* (bounced) key events to stderr
    --list-devices            # List available input devices and their capabilities (requires root)
    --stats-json              # Output statistics as JSON format to stderr on exit and periodic dump
    --verbose                 # Enable verbose logging (internal state, thread startup, etc)
    --ring-buffer-size: string # Size of the ring buffer for storing recently passed events (for debugging). Set to 0 to disable. (Default: 0)
    --ignore-key: string      # Key codes or names to ignore (never debounce). Example: `--ignore-key 114` or `--ignore-key KEY_VOLUMEDOWN`
    --otel-endpoint: string   # OTLP endpoint URL for exporting traces and metrics (e.g., "http://localhost:4317")
    --help(-h)                # Print help (see more with '--help')
    --version(-V)             # Print version
  ]

}

export use completions *
