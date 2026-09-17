#!/usr/bin/env bash
# Build release binaries against an old glibc so they run on any current
# distribution. Built on the developer's own machine they required glibc 2.43
# (uscreen-gui) and 2.39 (uscreen), which rules out every Debian and Ubuntu
# release in use. Debian 12 has 2.36, and anything built there runs on
# everything newer.
#
# Needs a distrobox container named "uscreen-build" made from debian:12 with
# build-essential, pkg-config, libdrm-dev, the X11/Wayland dev packages for
# the GUI, git, dpkg-dev, fakeroot, rpm and rustup.
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

for b in target-deb12/release/uscreen target-deb12/release/uscreen-gui target-deb12/evdi_helper; do
  need="$(objdump -T "$b" | grep -oE 'GLIBC_[0-9.]+' | sort -Vu | tail -1)"
  echo "  $(basename "$b"): needs $need"
  case "$need" in GLIBC_2.3[0-6]|GLIBC_2.[0-2]*|GLIBC_2.[0-9]) ;; *) echo "  !! $b needs $need — not portable"; exit 1;; esac
done

# Same layout as `make dist-local`, from the portable binaries.
D="dist/uscreen-$VERSION"
rm -rf "$D"; mkdir -p "$D/bin" "$D/scripts" "$D/packaging"
cp target-deb12/release/uscreen target-deb12/release/uscreen-gui target-deb12/evdi_helper "$D/bin/"
cp target-deb12/evdi-src/library/libevdi.so.1.15.0 "$D/bin/"
ln -sf libevdi.so.1.15.0 "$D/bin/libevdi.so.1"
cp scripts/install.sh scripts/write-desktop-entry.sh scripts/uscreen.desktop scripts/uscreen.service scripts/copy-distribution-docs.sh "$D/scripts/"
cp packaging/distribution-docs.txt packaging/uscreen-evdi.conf packaging/uscreen-modules.conf packaging/uscreen.service packaging/60-uscreen-uinput.rules "$D/packaging/"
mkdir -p "$D/packaging/icons" && cp packaging/icons/uscreen.svg packaging/icons/uscreen-pen.svg "$D/packaging/icons/"
./scripts/copy-distribution-docs.sh "$D/"
( cd android && ./gradlew assembleRelease -q && cp app/build/outputs/apk/release/app-release.apk "../$D/uscreen.apk" )
tar -C dist -czf "dist/uscreen-$VERSION-linux-x86_64.tar.gz" "uscreen-$VERSION"

echo "  helper runpath: $(readelf -d "$D/bin/evdi_helper" | grep -i runpath | grep -oE "\[.*\]")"
echo "✓ dist/uscreen-$VERSION-linux-x86_64.tar.gz (portable)"
