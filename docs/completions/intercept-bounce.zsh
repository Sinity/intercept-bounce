#compdef intercept-bounce

autoload -U is-at-least

_intercept-bounce() {
    typeset -A opt_args
    typeset -a _arguments_options
    local ret=1

    if is-at-least 5.2; then
        _arguments_options=(-s -S -C)
    else
        _arguments_options=(-s -C)
    fi

    local context curcontext="$curcontext" state line
    _arguments "${_arguments_options[@]}" : \
'-t+[Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default\: 25ms). The "value" refers to the state of the key\: \`1\` for press, \`0\` for release, \`2\` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"]:DEBOUNCE_TIME:_default' \
'--debounce-time=[Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default\: 25ms). The "value" refers to the state of the key\: \`1\` for press, \`0\` for release, \`2\` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"]:DEBOUNCE_TIME:_default' \
'--near-miss-threshold-time=[Threshold for logging "near-miss" events. Passed key events occurring within this time of the previous passed event are logged/counted. (Default\: 100ms) Accepts values like "100ms", "0.1s"]:NEAR_MISS_THRESHOLD_TIME:_default' \
'--log-interval=[Periodically dump statistics to stderr. (Default\: 15m). Set to "0" to disable periodic dumps. Accepts values like "60s", "15m", "1h"]:LOG_INTERVAL:_default' \
'--ring-buffer-size=[Size of the ring buffer for storing recently passed events (for debugging). Set to 0 to disable. (Default\: 0)]:RING_BUFFER_SIZE:_default' \
'*--debounce-key=[Key codes or names to debounce. When present, only these keys are debounced (all others pass through). Takes precedence over \`--ignore-key\`. Example\: \`--debounce-key KEY_ENTER\` (repeat flag for multiple keys)]:KEY:_default' \
'*--ignore-key=[Key codes or names to ignore (never debounce) unless they also appear in \`--debounce-key\`. Example\: \`--ignore-key 114\` or \`--ignore-key KEY_VOLUMEDOWN\`]:KEY:_default' \
'--otel-endpoint=[OTLP endpoint URL for exporting traces and metrics (e.g., "http\://localhost\:4317")]:OTEL_ENDPOINT:_default' \
'--log-all-events[Log details of *every* incoming event to stderr (\[PASS\] or \[DROP\])]' \
'--log-bounces[Log details of *only dropped* (bounced) key events to stderr]' \
'--list-devices[List available input devices and their capabilities (requires root)]' \
'--stats-json[Output statistics as JSON format to stderr on exit and periodic dump]' \
'--verbose[Enable verbose logging (internal state, thread startup, etc)]' \
'-h[Print help (see more with '\''--help'\'')]' \
'--help[Print help (see more with '\''--help'\'')]' \
'-V[Print version]' \
'--version[Print version]' \
&& ret=0
}

(( $+functions[_intercept-bounce_commands] )) ||
_intercept-bounce_commands() {
    local commands; commands=()
    _describe -t commands 'intercept-bounce commands' commands "$@"
}

if [ "$funcstack[1]" = "_intercept-bounce" ]; then
    _intercept-bounce "$@"
else
    compdef _intercept-bounce intercept-bounce
fi
