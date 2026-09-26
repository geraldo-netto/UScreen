# Source through BASH_ENV only during isolated tests. Record locations and source
# hashes, never command arguments, environment values or expanded credentials.
if [[ -n ${BLENT_SHELL_COVERAGE_DIR-} ]]; then
    /usr/bin/mkdir -p -- "$BLENT_SHELL_COVERAGE_DIR"
    exec {blent_coverage_fd}>>"$BLENT_SHELL_COVERAGE_DIR/trace-$BASHPID-$RANDOM.bin"
    blent_coverage_source=
    blent_coverage_hash=
    blent_coverage_hook=${BASH_SOURCE[0]}

    blent_coverage_origin() {
        if [[ -z ${BLENT_SHELL_COVERAGE_ORIGINS-} || -z ${BLENT_SHELL_COVERAGE_PYTHON-} ]]; then
            return 0
        fi
        if [[ -f $BLENT_SHELL_COVERAGE_DIR/origins/$blent_coverage_hash.json ]]; then
            return 0
        fi
        if ! "$BLENT_SHELL_COVERAGE_PYTHON" "$BLENT_SHELL_COVERAGE_ORIGINS" \
            "$blent_coverage_source" "$blent_coverage_hash"; then
            # Refuse the final report, without changing the tested command's status.
            printf 'origin attestation failed\n' >"$BLENT_SHELL_COVERAGE_DIR/origins.failed"
        fi
    }

    blent_coverage_location() {
        local source=${BASH_SOURCE[1]-} line=${BASH_LINENO[0]-0} checksum
        if [[ -z $source || $source == "$blent_coverage_hook" || ! -f $source ]]; then
            return 0
        fi
        if [[ $source != "$blent_coverage_source" ]]; then
            checksum=$(/usr/bin/sha256sum -- "$source") || return 0
            blent_coverage_source=$source
            blent_coverage_hash=${checksum%% *}
            # sha256sum prefixes escaped filenames with a backslash.
            blent_coverage_hash=${blent_coverage_hash#\\}
            blent_coverage_origin
        fi
        printf '%s\0%s\0%s\0' "$blent_coverage_hash" "$source" "$line" >&"$blent_coverage_fd" || :
        return 0
    }
    set -T
    trap 'blent_coverage_location' DEBUG
fi
