#!/usr/bin/env bash
# T308: clean-container runtime closure before installing any GUI test packages.
set -euo pipefail
[[ -f /.dockerenv ]]
ROOT=$(mktemp -d '/tmp/uscreen image.XXXXXXXX')
trap 'rm -rf "$ROOT"' EXIT
cp /artifacts/*.AppImage "$ROOT/UScreen.AppImage"
chmod +x "$ROOT/UScreen.AppImage"
cd "$ROOT"
./UScreen.AppImage --appimage-extract >/dev/null
APP="$ROOT/squashfs-root"
export PATH="$APP/usr/bin:$PATH"
for binary in uscreen uscreen-gui evdi_helper bash; do
    loaded=$(ldd "$APP/usr/bin/$binary")
    printf '%s\n' "$loaded"
    if grep -q 'not found' <<< "$loaded"; then exit 1; fi
done
loaded_evdi=$(ldd "$APP/usr/bin/evdi_helper" | awk '/libevdi.so.1 =>/ { sub(/^.*=> /, ""); sub(/ \(0x.*$/, ""); print }')
[[ $(readlink -f "$loaded_evdi") == "$APP/usr/bin/libevdi.so.1.15.0" ]]
"$APP/AppRun" --daemon --version
"$APP/AppRun" status
./UScreen.AppImage --appimage-extract-and-run status
ffmpeg -hide_banner -version
ffprobe -hide_banner -version
adb version
ffmpeg -hide_banner -loglevel error -f lavfi -i testsrc2=size=64x64:rate=1 -frames:v 2 -c:v libx264 -f null -
for file in LICENSE THIRD_PARTY_LICENSES.md licenses/libevdi-LGPL-2.1.txt bundled/manifest.json; do
    test -s "$APP/usr/share/doc/uscreen/$file"
done
if "$APP/usr/bin/evdi_helper" > "$ROOT/helper" 2>&1; then exit 1; else [[ $? == 1 ]]; fi
grep -q 'Usage:' "$ROOT/helper"
# Registration uses a stable outer image even in extraction mode.
export HOME="$ROOT/home" XDG_CONFIG_HOME="$ROOT/config" XDG_DATA_HOME="$ROOT/data"
./UScreen.AppImage --appimage-extract-and-run --install-user
"$HOME/.local/bin/uscreen" status
# The existing idle-daemon and GUI tests use only this extracted distribution.
mkdir -p "$ROOT/commands"
ln -s "$APP/AppRun" "$ROOT/commands/uscreen"
printf '#!/bin/sh\nexec "%s/AppRun" --gui "$@"\n' "$APP" > "$ROOT/commands/uscreen-gui"
chmod +x "$ROOT/commands/uscreen-gui"
export PATH="$ROOT/commands:$PATH"
/source/scripts/ci/direct-control-smoke.sh
# Xvfb and Mesa are host graphics/test prerequisites, not bundle dependencies.
apt-get update
apt-get install -y --no-install-recommends xvfb x11-utils libgl1-mesa-dri libglx-mesa0 libegl-mesa0
/source/scripts/ci/gui-smoke.sh
/source/scripts/ci/appimage-lifetime-smoke.sh "$ROOT/UScreen.AppImage"
