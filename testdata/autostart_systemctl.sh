#!/bin/sh
# T536: reachable user manager whose desktop never activates the graphical target.
set -eu
printf '%s\n' "$*" >> "$BLENT_T536_STATE/calls"
case "$*" in
    '--user show -p LoadState --value blent.service') echo loaded ;;
    '--user is-active graphical-session.target') echo inactive; exit 3 ;;
    '--user daemon-reload') ;;
    '--user enable blent.service') touch "$BLENT_T536_STATE/enabled" ;;
    '--user disable blent.service') rm -f "$BLENT_T536_STATE/enabled" ;;
    '--user is-enabled blent.service'|'--user is-enabled --quiet blent.service')
        test -f "$BLENT_T536_STATE/enabled" || exit 1
        echo enabled ;;
    '--user start blent.service')
        if mkdir "$BLENT_T536_STATE/running" 2>/dev/null; then
            printf 'daemon\n' >> "$BLENT_T536_STATE/launches"
        fi ;;
    '--user stop blent.service') rmdir "$BLENT_T536_STATE/running" ;;
    *) echo "Unexpected systemctl arguments: $*" >&2; exit 2 ;;
esac
