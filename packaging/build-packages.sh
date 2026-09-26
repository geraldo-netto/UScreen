#!/usr/bin/env bash
# Build the AppImage (with sources) and .rpm from an already-built portable dist tree
# (scripts/build-release.sh). Runs inside the Debian 12 build container,
# with AppImage tooling, Debian source repositories and rpmbuild.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(sed -n 's/^VERSION = //p' Makefile)"
D="dist/blent-$VERSION"
[ -x "$D/bin/blent" ] || { echo "run scripts/build-release.sh first"; exit 1; }

# Remove prior outputs and success marker before entering the container.
rm -f "dist/blent-$VERSION-x86_64.AppImage" "dist/blent-$VERSION-AppImage-sources.tar.gz" "dist/blent-$VERSION-"*.rpm \
      "dist/blent-$VERSION-PKGBUILD.tar.gz" dist/.packages-ok

distrobox enter "${BLENT_BUILD_CONTAINER:-blent-build}" -- bash -lc '
  set -euo pipefail
  cd "$1"
  V="$2"; D="$3"

  python3 packaging/appimage/build.py --bundle "$D" --output dist --version "$V" \
    --evdi-source "${BLENT_EVDI_SOURCE:-target-deb12/evdi-src}"

  # ---- .rpm ----
  # RPM expands paths into shell programs; use a space-free topdir.
  RB=$(mktemp -d /tmp/blent-rpmbuild.XXXXXXXX)
  trap "rm -rf \"$RB\"" EXIT
  mkdir -p "$RB"/{SOURCES,SPECS,BUILD,RPMS,SRPMS}
  cp dist/blent-$V-linux-x86_64.tar.gz "$RB/SOURCES/"
  sed "s/^Version:.*/Version:        $V/" packaging/rpm/blent.spec > "$RB/SPECS/blent.spec"
  rpmbuild --define "_topdir $RB" --define "_userunitdir /usr/lib/systemd/user" --define "_libdir /usr/lib64" \
           --define "_modprobedir /usr/lib/modprobe.d" --define "_modulesloaddir /usr/lib/modules-load.d" --define "_udevrulesdir /usr/lib/udev/rules.d" \
           -bb "$RB/SPECS/blent.spec" 2>&1 | tee "$RB/build.log"
  cp "$RB/RPMS/x86_64/blent-$V-"*.rpm dist/
  touch dist/.packages-ok
' blent-packages "$PWD" "$VERSION" "$D"
[ -f dist/.packages-ok ] || { echo "!! package build inside container failed"; exit 1; }
rm -f dist/.packages-ok
[ -s "dist/blent-$VERSION-x86_64.AppImage" ] && [ -s "dist/blent-$VERSION-AppImage-sources.tar.gz" ] && [ -s "dist/blent-$VERSION-1.x86_64.rpm" ] \
  || { echo "!! package outputs missing"; exit 1; }

# Arch users get the PKGBUILD as a release file too; makepkg needs the
# install script next to it, hence a small archive rather than a bare file.
# pkgver is rewritten like the deb/rpm versions so the archive can never
# point at an older tag than the release it ships with.
ARCH_TMP=$(mktemp -d); trap 'rm -rf "$ARCH_TMP"' EXIT
sed "s/^pkgver=.*/pkgver=$VERSION/" packaging/arch/PKGBUILD > "$ARCH_TMP/PKGBUILD"
cp packaging/arch/blent.install "$ARCH_TMP/"
tar -C "$ARCH_TMP" -czf "dist/blent-$VERSION-PKGBUILD.tar.gz" PKGBUILD blent.install

ls -la dist/*.AppImage dist/*AppImage-sources.tar.gz dist/*.rpm dist/*PKGBUILD*
