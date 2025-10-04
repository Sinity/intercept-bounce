_intercept-bounce() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="intercept__bounce"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        intercept__bounce)
            opts="-t -h -V --debounce-time --near-miss-threshold-time --log-interval --log-all-events --log-bounces --list-devices --stats-json --verbose --ring-buffer-size --debounce-key --ignore-key --otel-endpoint --help --version"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --debounce-time)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                -t)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --near-miss-threshold-time)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --log-interval)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --ring-buffer-size)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --debounce-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --ignore-key)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --otel-endpoint)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _intercept-bounce -o nosort -o bashdefault -o default intercept-bounce
else
    complete -F _intercept-bounce -o bashdefault -o default intercept-bounce
fi
