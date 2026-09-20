#!/bin/sh
# T536: reachable user manager whose desktop never activates the graphical target.
set -eu
printf '%s\n' "$*" >> "$USCREEN_T536_STATE/calls"
case "$*" in
    '--user show -p LoadState --value uscreen.service') echo loaded ;;
    '--user is-active graphical-session.target') echo inactive; exit 3 ;;
    '--user daemon-reload') ;;
    '--user enable uscreen.service') touch "$USCREEN_T536_STATE/enabled" ;;
    '--user disable uscreen.service') rm -f "$USCREEN_T536_STATE/enabled" ;;
    '--user is-enabled uscreen.service'|'--user is-enabled --quiet uscreen.service')
        test -f "$USCREEN_T536_STATE/enabled" || exit 1
        echo enabled ;;
    '--user start uscreen.service')
        if mkdir "$USCREEN_T536_STATE/running" 2>/dev/null; then
            printf 'daemon\n' >> "$USCREEN_T536_STATE/launches"
        fi ;;
    '--user stop uscreen.service') rmdir "$USCREEN_T536_STATE/running" ;;
    *) echo "Unexpected systemctl arguments: $*" >&2; exit 2 ;;
esac
