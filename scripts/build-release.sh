#!/usr/bin/env bash
# Build Linux x86-64 binaries against Debian 12 and validate glibc <= 2.36.
# Local builds can require newer glibc versions. This ABI check does not
# establish runtime dependency, GPU, kernel or compositor compatibility.
#
# Needs a distrobox container named "uscreen-build" made from debian:12 with
# build-essential, pkg-config, libdrm-dev, the X11/Wayland dev packages for
# the GUI, git, dpkg-dev, fakeroot, rpm and rustup.
# The host also needs Python 3 and binutils for the bundle ABI check.
set -euo pipefail
cd "$(dirname "$0")/.."
CONTAINER="${USCREEN_BUILD_CONTAINER:-uscreen-build}"
VERSION="$(sed -n 's/^VERSION = //p' Makefile)"
EVDI_TAG="v1.15.0"

# Verify completion even when distrobox does not propagate failure.
rm -f target-deb12/.build-ok
distrobox enter "$CONTAINER" -- bash -lc '
  set -euo pipefail
  export PATH="$HOME/.cargo/bin:$PATH"
  cd "$1"
  export CARGO_TARGET_DIR="$PWD/target-deb12"
  cargo build --release --locked --manifest-path host/Cargo.toml
  cargo build --release --locked --manifest-path gui/Cargo.toml

  # Keep LGPL libevdi replaceable beside the helper, located via $ORIGIN.
  [ -d target-deb12/evdi-src ] || git clone -q --depth 1 --branch "$2" https://github.com/DisplayLink/evdi target-deb12/evdi-src
  make -s -C target-deb12/evdi-src/library >/dev/null
  gcc -O3 -Ihost/evdi -o target-deb12/evdi_helper host/evdi/evdi_helper.c \
      -Ltarget-deb12/evdi-src/library -levdi -lpthread "-Wl,-rpath,\$ORIGIN"
  touch target-deb12/.build-ok
' uscreen-release "$PWD" "$EVDI_TAG"
[ -f target-deb12/.build-ok ] || { echo "!! build inside $CONTAINER failed"; exit 1; }

# Same layout as `make dist-local`, from the portable binaries.
D="dist/uscreen-$VERSION"
rm -rf "$D"
./scripts/stage-linux-bundle.sh target-deb12/release target-deb12/evdi_helper target-deb12/evdi-src/library "$D"
python3 scripts/ci/verify-portability.py "$D/bin"
( cd android && ./gradlew assembleRelease -q && cp app/build/outputs/apk/release/app-release.apk "../$D/uscreen.apk" )
tar -C dist -czf "dist/uscreen-$VERSION-linux-x86_64.tar.gz" "uscreen-$VERSION"

echo "  helper runpath: $(readelf -d "$D/bin/evdi_helper" | grep -i runpath | grep -oE "\[.*\]")"
echo "✓ dist/uscreen-$VERSION-linux-x86_64.tar.gz (portable)"
