#!/usr/bin/env bash
# Encode Exec according to the Desktop Entry Specification, not shell syntax.
# https://specifications.freedesktop.org/desktop-entry/latest/exec-variables.html
set -euo pipefail
executable=$1
template=$2
[[ $executable = /* ]] || executable="$PWD/$executable"
escaped=
for ((i=0; i<${#executable}; i++)); do
    char=${executable:i:1}
    case "$char" in
        '\'|'"'|'$'|'`') escaped+="\\$char" ;;
        '%') escaped+='%%' ;;
        *) escaped+="$char" ;;
    esac
done
# The desktop string layer is decoded before Exec quoting.
escaped=${escaped//\\/\\\\}
escaped=${escaped//$'\n'/\\n}
escaped=${escaped//$'\r'/\\r}
escaped=${escaped//$'\t'/\\t}
while IFS= read -r line || [[ -n $line ]]; do
    if [[ $line = Exec=* ]]; then
        # GIO checks the executable before expanding %% field escapes. Keep
        # the executable fixed and pass the user path as an encoded argument.
        printf 'Exec=/usr/bin/env "%s"\n' "$escaped"
    else
        printf '%s\n' "$line"
    fi
done < "$template"
