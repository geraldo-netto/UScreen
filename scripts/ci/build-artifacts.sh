#!/usr/bin/env bash
# Linux package fixtures from real portable binaries; no Android placeholder APK.
set -euo pipefail
cd "$(dirname "$0")/../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target-portability}"
cargo build --locked --release --workspace
make build-helper
VERSION=$(sed -n 's/^VERSION = //p' Makefile)
D="dist/uscreen-$VERSION"
rm -rf "$D"
./scripts/stage-linux-bundle.sh "$CARGO_TARGET_DIR/release" host/evdi/evdi_helper /opt/evdi/library "$D"
python3 scripts/ci/verify-portability.py "$D/bin"
tar -C dist -czf "dist/uscreen-$VERSION-linux-x86_64.tar.gz" "uscreen-$VERSION"
# Already inside the build container: execute the package program locally.
export PATH="$PWD/scripts/ci:$PATH"
bash packaging/build-packages.sh
