
use builtin;
use str;

set edit:completion:arg-completer[intercept-bounce] = {|@words|
    fn spaces {|n|
        builtin:repeat $n ' ' | str:join ''
    }
    fn cand {|text desc|
        edit:complex-candidate $text &display=$text' '(spaces (- 14 (wcswidth $text)))$desc
    }
    var command = 'intercept-bounce'
    for word $words[1..-1] {
        if (str:has-prefix $word '-') {
            break
        }
        set command = $command';'$word
    }
    var completions = [
        &'intercept-bounce'= {
            cand -t 'Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default: 25ms). The "value" refers to the state of the key: `1` for press, `0` for release, `2` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"'
            cand --debounce-time 'Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default: 25ms). The "value" refers to the state of the key: `1` for press, `0` for release, `2` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"'
            cand --near-miss-threshold-time 'Threshold for logging "near-miss" events. Passed key events occurring within this time of the previous passed event are logged/counted. (Default: 100ms) Accepts values like "100ms", "0.1s"'
            cand --log-interval 'Periodically dump statistics to stderr. (Default: 15m). Set to "0" to disable periodic dumps. Accepts values like "60s", "15m", "1h"'
            cand --ring-buffer-size 'Size of the ring buffer for storing recently passed events (for debugging). Set to 0 to disable. (Default: 0)'
            cand --debounce-key 'Key codes or names to debounce. When present, only these keys are debounced (all others pass through). Takes precedence over `--ignore-key`. Example: `--debounce-key KEY_ENTER` (repeat flag for multiple keys)'
            cand --ignore-key 'Key codes or names to ignore (never debounce) unless they also appear in `--debounce-key`. Example: `--ignore-key 114` or `--ignore-key KEY_VOLUMEDOWN`'
            cand --otel-endpoint 'OTLP endpoint URL for exporting traces and metrics (e.g., "http://localhost:4317")'
            cand --log-all-events 'Log details of *every* incoming event to stderr ([PASS] or [DROP])'
            cand --log-bounces 'Log details of *only dropped* (bounced) key events to stderr'
            cand --list-devices 'List available input devices and their capabilities (requires root)'
            cand --stats-json 'Output statistics as JSON format to stderr on exit and periodic dump'
            cand --verbose 'Enable verbose logging (internal state, thread startup, etc)'
            cand -h 'Print help (see more with ''--help'')'
            cand --help 'Print help (see more with ''--help'')'
            cand -V 'Print version'
            cand --version 'Print version'
        }
    ]
    $completions[$command]
}
