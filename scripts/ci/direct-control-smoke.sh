#!/usr/bin/env bash
# T236: real idle daemon lifecycle with no systemd, display devices or user bus.
set -euo pipefail
[[ -f /.dockerenv && $(cat /proc/1/comm) != systemd ]]
unset DBUS_SESSION_BUS_ADDRESS
SMOKE_ROOT=$(mktemp -d)
export XDG_CONFIG_HOME="$SMOKE_ROOT/config"
mkdir -p "$XDG_CONFIG_HOME/uscreen"
cat > "$XDG_CONFIG_HOME/uscreen/config.toml" <<'CONFIG'
input_touch = false
input_pen = false
input_pointer = false
check_updates = false
auto_launch_app = false
CONFIG
printf '#!/bin/sh\ntouch "$0.called"\nexit 97\n' > "$SMOKE_ROOT/helper"
chmod +x "$SMOKE_ROOT/helper"
uscreen --encoder libx264 --helper "$SMOKE_ROOT/helper" > "$SMOKE_ROOT/daemon.log" 2>&1 &
DAEMON_PID=$!
trap 'kill "$DAEMON_PID" 2>/dev/null || true; wait "$DAEMON_PID" 2>/dev/null || true; rm -rf "$SMOKE_ROOT"' EXIT
for attempt in {1..50}; do
    if ! kill -0 "$DAEMON_PID"; then cat "$SMOKE_ROOT/daemon.log"; exit 1; fi
    if grep -q "daemon running" "$SMOKE_ROOT/daemon.log"; then break; fi
    sleep 0.1
done
uscreen status | grep -F "running (PID: $DAEMON_PID)"
uscreen stop
wait "$DAEMON_PID"
[[ ! -e "$SMOKE_ROOT/helper.called" ]]
