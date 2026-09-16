#!/usr/bin/env bash
# Build the .deb and .rpm from an already-built portable dist tree
# (scripts/build-release.sh). Runs inside the Debian 12 build container,
# which has dpkg-deb and rpmbuild.
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION="$(sed -n 's/^VERSION = //p' Makefile)"
D="dist/uscreen-$VERSION"
[ -x "$D/bin/uscreen" ] || { echo "run scripts/build-release.sh first"; exit 1; }

# Remove prior outputs and success marker before entering the container.
rm -f "dist/uscreen_${VERSION}_amd64.deb" "dist/uscreen-$VERSION-"*.rpm \
      "dist/uscreen-$VERSION-PKGBUILD.tar.gz" dist/.packages-ok

distrobox enter "${USCREEN_BUILD_CONTAINER:-uscreen-build}" -- bash -lc '
  set -euo pipefail
  cd '"$PWD"'
  V='"$VERSION"'; D='"$D"'

  # ---- .deb ----
  R=dist/deb-root; rm -rf "$R"
  install -Dm755 $D/bin/uscreen        $R/usr/bin/uscreen
  install -Dm755 $D/bin/uscreen-gui    $R/usr/bin/uscreen-gui
  install -Dm755 $D/bin/evdi_helper    $R/usr/lib/uscreen/evdi_helper
  install -Dm755 $D/bin/libevdi.so.1.15.0 $R/usr/lib/uscreen/libevdi.so.1.15.0
  ln -sf libevdi.so.1.15.0 $R/usr/lib/uscreen/libevdi.so.1
  install -Dm644 scripts/uscreen.desktop $R/usr/share/applications/uscreen.desktop
  install -Dm644 packaging/icons/uscreen.svg     $R/usr/share/icons/hicolor/scalable/apps/uscreen.svg
  install -Dm644 packaging/icons/uscreen-pen.svg $R/usr/share/icons/hicolor/scalable/apps/uscreen-pen.svg
  install -Dm644 packaging/uscreen.service $R/usr/lib/systemd/user/uscreen.service
  install -Dm644 packaging/uscreen-evdi.conf    $R/usr/lib/modprobe.d/uscreen-evdi.conf
  install -Dm644 packaging/uscreen-modules.conf $R/usr/lib/modules-load.d/uscreen.conf
  install -Dm644 packaging/60-uscreen-uinput.rules $R/usr/lib/udev/rules.d/60-uscreen-uinput.rules
  ./scripts/copy-distribution-docs.sh "$R/usr/share/doc/uscreen"
  mkdir -p $R/DEBIAN
  sed "s/^Version: .*/Version: $V/" packaging/deb/control > $R/DEBIAN/control
  install -m755 packaging/deb/postinst $R/DEBIAN/postinst
  fakeroot dpkg-deb --build --root-owner-group $R dist/uscreen_${V}_amd64.deb
  dpkg-deb --info dist/uscreen_${V}_amd64.deb | grep -E "Package|Version|Depends"

  # ---- .rpm ----
  RB=$PWD/dist/rpmbuild; rm -rf $RB; mkdir -p $RB/{SOURCES,SPECS,BUILD,RPMS,SRPMS}
  cp dist/uscreen-$V-linux-x86_64.tar.gz $RB/SOURCES/
  sed "s/^Version:.*/Version:        $V/" packaging/rpm/uscreen.spec > $RB/SPECS/uscreen.spec
  rpmbuild --define "_topdir $RB" --define "_userunitdir /usr/lib/systemd/user" --define "_libdir /usr/lib64" \
           --define "_modprobedir /usr/lib/modprobe.d" --define "_modulesloaddir /usr/lib/modules-load.d" --define "_udevrulesdir /usr/lib/udev/rules.d" \
           -bb $RB/SPECS/uscreen.spec 2>&1 | tee "$RB/build.log"
  cp $RB/RPMS/x86_64/uscreen-$V-*.rpm dist/
  touch dist/.packages-ok
'
[ -f dist/.packages-ok ] || { echo "!! package build inside container failed"; exit 1; }
rm -f dist/.packages-ok
[ -s "dist/uscreen_${VERSION}_amd64.deb" ] && [ -s "dist/uscreen-$VERSION-1.x86_64.rpm" ] \
  || { echo "!! package outputs missing"; exit 1; }

# Arch users get the PKGBUILD as a release file too; makepkg needs the
# install script next to it, hence a small archive rather than a bare file.
# pkgver is rewritten like the deb/rpm versions so the archive can never
# point at an older tag than the release it ships with.
ARCH_TMP=$(mktemp -d); trap 'rm -rf "$ARCH_TMP"' EXIT
sed "s/^pkgver=.*/pkgver=$VERSION/" packaging/arch/PKGBUILD > "$ARCH_TMP/PKGBUILD"
cp packaging/arch/uscreen.install "$ARCH_TMP/"
tar -C "$ARCH_TMP" -czf "dist/uscreen-$VERSION-PKGBUILD.tar.gz" PKGBUILD uscreen.install

ls -la dist/*.deb dist/*.rpm dist/*PKGBUILD*
