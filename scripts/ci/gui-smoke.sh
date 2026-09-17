#!/usr/bin/env bash
# T314: dlopen dependencies are invisible to ldd; exercise the installed GUI.
# Called only inside native-package CI containers, using their own Xvfb.
set -euo pipefail
[[ -f /.dockerenv ]]
GUI_SMOKE_ROOT=$(mktemp -d)
GUI_PID=
XVFB_PID=
cleanup() {
    if [[ -n "$GUI_PID" ]]; then
        kill "$GUI_PID" 2>/dev/null || true
        wait "$GUI_PID" 2>/dev/null || true
    fi
    if [[ -n "$XVFB_PID" ]]; then
        kill "$XVFB_PID" 2>/dev/null || true
        wait "$XVFB_PID" 2>/dev/null || true
    fi
    rm -rf "$GUI_SMOKE_ROOT"
}
trap cleanup EXIT
mkdir -p "$GUI_SMOKE_ROOT/config/uscreen"
printf 'check_updates = false\n' > "$GUI_SMOKE_ROOT/config/uscreen/config.toml"
export XDG_CONFIG_HOME="$GUI_SMOKE_ROOT/config"
export WINIT_UNIX_BACKEND=x11 LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS
Xvfb -displayfd 3 -screen 0 1280x1024x24 -nolisten tcp \
    3> "$GUI_SMOKE_ROOT/display" > "$GUI_SMOKE_ROOT/xvfb.log" 2>&1 &
XVFB_PID=$!
for _ in {1..50}; do
    if [[ -s "$GUI_SMOKE_ROOT/display" ]]; then break; fi
    if ! kill -0 "$XVFB_PID" 2>/dev/null; then cat "$GUI_SMOKE_ROOT/xvfb.log"; exit 1; fi
    sleep 0.1
done
[[ -s "$GUI_SMOKE_ROOT/display" ]]
read -r display_number < "$GUI_SMOKE_ROOT/display"
export DISPLAY=:$display_number
uscreen-gui > "$GUI_SMOKE_ROOT/gui.log" 2>&1 &
GUI_PID=$!
sleep 3
cat "$GUI_SMOKE_ROOT/gui.log"
kill -0 "$GUI_PID"
xwininfo -root -tree > "$GUI_SMOKE_ROOT/windows"
cat "$GUI_SMOKE_ROOT/windows"
grep -q '"UScreen"' "$GUI_SMOKE_ROOT/windows"
if grep -q 'panicked' "$GUI_SMOKE_ROOT/gui.log"; then exit 1; fi
