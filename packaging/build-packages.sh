#!/usr/bin/env bash
# Build the AppImage (with sources) and .rpm from an already-built portable dist tree
# (scripts/build-release.sh). Runs inside the Debian 12 build container,
# with AppImage tooling, Debian source repositories and rpmbuild.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(sed -n 's/^VERSION = //p' Makefile)"
D="dist/uscreen-$VERSION"
[ -x "$D/bin/uscreen" ] || { echo "run scripts/build-release.sh first"; exit 1; }

# Remove prior outputs and success marker before entering the container.
rm -f "dist/uscreen-$VERSION-x86_64.AppImage" "dist/uscreen-$VERSION-AppImage-sources.tar.gz" "dist/uscreen-$VERSION-"*.rpm \
      "dist/uscreen-$VERSION-PKGBUILD.tar.gz" dist/.packages-ok

distrobox enter "${USCREEN_BUILD_CONTAINER:-uscreen-build}" -- bash -lc '
  set -euo pipefail
  cd "$1"
  V="$2"; D="$3"

  python3 packaging/appimage/build.py --bundle "$D" --output dist --version "$V" \
    --evdi-source "${USCREEN_EVDI_SOURCE:-target-deb12/evdi-src}"

  # ---- .rpm ----
  # RPM expands paths into shell programs; use a space-free topdir.
  RB=$(mktemp -d /tmp/uscreen-rpmbuild.XXXXXXXX)
  trap "rm -rf \"$RB\"" EXIT
  mkdir -p "$RB"/{SOURCES,SPECS,BUILD,RPMS,SRPMS}
  cp dist/uscreen-$V-linux-x86_64.tar.gz "$RB/SOURCES/"
  sed "s/^Version:.*/Version:        $V/" packaging/rpm/uscreen.spec > "$RB/SPECS/uscreen.spec"
  rpmbuild --define "_topdir $RB" --define "_userunitdir /usr/lib/systemd/user" --define "_libdir /usr/lib64" \
           --define "_modprobedir /usr/lib/modprobe.d" --define "_modulesloaddir /usr/lib/modules-load.d" --define "_udevrulesdir /usr/lib/udev/rules.d" \
           -bb "$RB/SPECS/uscreen.spec" 2>&1 | tee "$RB/build.log"
  cp "$RB/RPMS/x86_64/uscreen-$V-"*.rpm dist/
  touch dist/.packages-ok
' uscreen-packages "$PWD" "$VERSION" "$D"
[ -f dist/.packages-ok ] || { echo "!! package build inside container failed"; exit 1; }
rm -f dist/.packages-ok
[ -s "dist/uscreen-$VERSION-x86_64.AppImage" ] && [ -s "dist/uscreen-$VERSION-AppImage-sources.tar.gz" ] && [ -s "dist/uscreen-$VERSION-1.x86_64.rpm" ] \
  || { echo "!! package outputs missing"; exit 1; }

# Arch users get the PKGBUILD as a release file too; makepkg needs the
# install script next to it, hence a small archive rather than a bare file.
# pkgver is rewritten like the deb/rpm versions so the archive can never
# point at an older tag than the release it ships with.
ARCH_TMP=$(mktemp -d); trap 'rm -rf "$ARCH_TMP"' EXIT
sed "s/^pkgver=.*/pkgver=$VERSION/" packaging/arch/PKGBUILD > "$ARCH_TMP/PKGBUILD"
cp packaging/arch/uscreen.install "$ARCH_TMP/"
tar -C "$ARCH_TMP" -czf "dist/uscreen-$VERSION-PKGBUILD.tar.gz" PKGBUILD uscreen.install

ls -la dist/*.AppImage dist/*AppImage-sources.tar.gz dist/*.rpm dist/*PKGBUILD*
