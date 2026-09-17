#!/usr/bin/env bash
# Shared Linux bundle layout; callers own build provenance, ABI checks and APKs.
# Arguments: Rust binary directory, helper executable, libevdi file, destination.
set -euo pipefail
BINARIES=$1
HELPER=$2
LIBRARY=$3
DEST=$4
mkdir -p "$DEST/bin" "$DEST/scripts" "$DEST/packaging/icons"
cp "$BINARIES/uscreen" "$BINARIES/uscreen-gui" "$HELPER" "$DEST/bin/"
cp -L "$LIBRARY" "$DEST/bin/libevdi.so.1.15.0"
ln -sf libevdi.so.1.15.0 "$DEST/bin/libevdi.so.1"
cp scripts/install.sh scripts/setup-evdi.sh scripts/write-desktop-entry.sh scripts/uscreen.desktop scripts/uscreen-autostart.desktop scripts/uscreen.service scripts/copy-distribution-docs.sh "$DEST/scripts/"
cp packaging/distribution-docs.txt packaging/uscreen-evdi.conf packaging/uscreen-modules.conf packaging/uscreen.service packaging/60-uscreen-uinput.rules "$DEST/packaging/"
cp packaging/icons/uscreen.svg packaging/icons/uscreen-pen.svg "$DEST/packaging/icons/"
./scripts/copy-distribution-docs.sh "$DEST/"
