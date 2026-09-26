#!/usr/bin/env bash
# T308: two actual extraction runtimes; retiring the GUI must preserve the daemon.
set -euo pipefail
[[ -f /.dockerenv ]]
IMAGE=$(readlink -f -- "$1")
ROOT=$(mktemp -d)
DAEMON= GUI= SERVER=
cleanup() {
    "$IMAGE" --appimage-extract-and-run stop >/dev/null 2>&1 || true
    for pid in "$GUI" "$DAEMON" "$SERVER"; do
        [[ -z $pid ]] || kill "$pid" 2>/dev/null || true
        [[ -z $pid ]] || wait "$pid" 2>/dev/null || true
    done
    rm -rf "$ROOT"
}
trap cleanup EXIT
export XDG_CONFIG_HOME="$ROOT/config" LIBGL_ALWAYS_SOFTWARE=1 WINIT_UNIX_BACKEND=x11
unset DBUS_SESSION_BUS_ADDRESS WAYLAND_DISPLAY
mkdir -p "$XDG_CONFIG_HOME/blent"
printf 'check_updates = false\ninput_touch = false\ninput_pen = false\ninput_pointer = false\nauto_launch_app = false\n' > "$XDG_CONFIG_HOME/blent/config.toml"
printf '#!/bin/sh\ntouch "$0.called"\nexit 97\n' > "$ROOT/helper"
chmod +x "$ROOT/helper"
"$IMAGE" --appimage-extract-and-run --daemon --encoder libx264 --helper "$ROOT/helper" > "$ROOT/daemon.log" 2>&1 &
DAEMON=$!
for _ in {1..100}; do
    kill -0 "$DAEMON"
    if grep -q 'daemon running' "$ROOT/daemon.log"; then break; fi
    sleep 0.1
done
grep -q 'daemon running' "$ROOT/daemon.log"
Xvfb -displayfd 3 -screen 0 1280x1024x24 -nolisten tcp 3> "$ROOT/display" > "$ROOT/xvfb.log" 2>&1 &
SERVER=$!
for _ in {1..50}; do [[ ! -s "$ROOT/display" ]] || break; sleep 0.1; done
read -r display < "$ROOT/display"
export DISPLAY=:$display
"$IMAGE" --appimage-extract-and-run --gui > "$ROOT/gui.log" 2>&1 &
GUI=$!
for _ in {1..100}; do
    kill -0 "$GUI"
    if xwininfo -root -tree | grep -q '"Blent"'; then break; fi
    sleep 0.1
done
xwininfo -root -tree | grep '"Blent"'
kill "$GUI"
wait "$GUI" || true
GUI=
kill -0 "$DAEMON"
"$IMAGE" --appimage-extract-and-run status | grep 'running (PID:'
"$IMAGE" --appimage-extract-and-run stop
wait "$DAEMON"
DAEMON=
[[ ! -e "$ROOT/helper.called" ]]
