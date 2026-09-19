#!/usr/bin/env bash
# Shared quoting for native and AppImage user units. No shell expansion in Exec.
set -euo pipefail
systemd_path() {
    local value=$1
    [[ $value = /* ]] || value="$PWD/$value"
    value=${value//\\/\\\\}
    value=${value//\"/\\\"}
    value=${value//%/%%}
    value=${value//$'\n'/\\n}
    value=${value//$'\r'/\\r}
    value=${value//$'\t'/\\t}
    printf '"%s"' "$value"
}
daemon=$(systemd_path "$1")
template=$2
helper=
if [[ $# -eq 3 ]]; then helper=" --helper $(systemd_path "$3")"; fi
while IFS= read -r line || [[ -n $line ]]; do
    case "$line" in
        ExecStart=*) printf 'ExecStart=:/usr/bin/env %s%s start\n' "$daemon" "$helper" ;;
        ExecStop=*) printf 'ExecStop=:/usr/bin/env %s stop\n' "$daemon" ;;
        *) printf '%s\n' "$line" ;;
    esac
done < "$template"
