# Source through BASH_ENV only during isolated tests. Record locations and source
# hashes, never command arguments, environment values or expanded credentials.
if [[ -n ${USCREEN_SHELL_COVERAGE_DIR-} ]]; then
    /usr/bin/mkdir -p -- "$USCREEN_SHELL_COVERAGE_DIR"
    exec {uscreen_coverage_fd}>>"$USCREEN_SHELL_COVERAGE_DIR/trace-$BASHPID-$RANDOM.bin"
    uscreen_coverage_source=
    uscreen_coverage_hash=
    uscreen_coverage_hook=${BASH_SOURCE[0]}

    uscreen_coverage_origin() {
        if [[ -z ${USCREEN_SHELL_COVERAGE_ORIGINS-} || -z ${USCREEN_SHELL_COVERAGE_PYTHON-} ]]; then
            return 0
        fi
        if [[ -f $USCREEN_SHELL_COVERAGE_DIR/origins/$uscreen_coverage_hash.json ]]; then
            return 0
        fi
        if ! "$USCREEN_SHELL_COVERAGE_PYTHON" "$USCREEN_SHELL_COVERAGE_ORIGINS" \
            "$uscreen_coverage_source" "$uscreen_coverage_hash"; then
            # Refuse the final report, without changing the tested command's status.
            printf 'origin attestation failed\n' >"$USCREEN_SHELL_COVERAGE_DIR/origins.failed"
        fi
    }

    uscreen_coverage_location() {
        local source=${BASH_SOURCE[1]-} line=${BASH_LINENO[0]-0} checksum
        if [[ -z $source || $source == "$uscreen_coverage_hook" || ! -f $source ]]; then
            return 0
        fi
        if [[ $source != "$uscreen_coverage_source" ]]; then
            checksum=$(/usr/bin/sha256sum -- "$source") || return 0
            uscreen_coverage_source=$source
            uscreen_coverage_hash=${checksum%% *}
            # sha256sum prefixes escaped filenames with a backslash.
            uscreen_coverage_hash=${uscreen_coverage_hash#\\}
            uscreen_coverage_origin
        fi
        printf '%s\0%s\0%s\0' "$uscreen_coverage_hash" "$source" "$line" >&"$uscreen_coverage_fd" || :
        return 0
    }
    set -T
    trap 'uscreen_coverage_location' DEBUG
fi
