
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'intercept-bounce' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'intercept-bounce'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'intercept-bounce' {
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default: 25ms). The "value" refers to the state of the key: `1` for press, `0` for release, `2` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"')
            [CompletionResult]::new('--debounce-time', '--debounce-time', [CompletionResultType]::ParameterName, 'Debounce time threshold (milliseconds). Duplicate key events (same keycode and value) occurring faster than this threshold are discarded. (Default: 25ms). The "value" refers to the state of the key: `1` for press, `0` for release, `2` for repeat. Only press and release events are debounced. Accepts values like "10ms", "0.5s"')
            [CompletionResult]::new('--near-miss-threshold-time', '--near-miss-threshold-time', [CompletionResultType]::ParameterName, 'Threshold for logging "near-miss" events. Passed key events occurring within this time of the previous passed event are logged/counted. (Default: 100ms) Accepts values like "100ms", "0.1s"')
            [CompletionResult]::new('--log-interval', '--log-interval', [CompletionResultType]::ParameterName, 'Periodically dump statistics to stderr. (Default: 15m). Set to "0" to disable periodic dumps. Accepts values like "60s", "15m", "1h"')
            [CompletionResult]::new('--ring-buffer-size', '--ring-buffer-size', [CompletionResultType]::ParameterName, 'Size of the ring buffer for storing recently passed events (for debugging). Set to 0 to disable. (Default: 0)')
            [CompletionResult]::new('--debounce-key', '--debounce-key', [CompletionResultType]::ParameterName, 'Key codes or names to debounce. When present, only these keys are debounced (all others pass through). Takes precedence over `--ignore-key`. Example: `--debounce-key KEY_ENTER` (repeat flag for multiple keys)')
            [CompletionResult]::new('--ignore-key', '--ignore-key', [CompletionResultType]::ParameterName, 'Key codes or names to ignore (never debounce) unless they also appear in `--debounce-key`. Example: `--ignore-key 114` or `--ignore-key KEY_VOLUMEDOWN`')
            [CompletionResult]::new('--otel-endpoint', '--otel-endpoint', [CompletionResultType]::ParameterName, 'OTLP endpoint URL for exporting traces and metrics (e.g., "http://localhost:4317")')
            [CompletionResult]::new('--log-all-events', '--log-all-events', [CompletionResultType]::ParameterName, 'Log details of *every* incoming event to stderr ([PASS] or [DROP])')
            [CompletionResult]::new('--log-bounces', '--log-bounces', [CompletionResultType]::ParameterName, 'Log details of *only dropped* (bounced) key events to stderr')
            [CompletionResult]::new('--list-devices', '--list-devices', [CompletionResultType]::ParameterName, 'List available input devices and their capabilities (requires root)')
            [CompletionResult]::new('--stats-json', '--stats-json', [CompletionResultType]::ParameterName, 'Output statistics as JSON format to stderr on exit and periodic dump')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Enable verbose logging (internal state, thread startup, etc)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
