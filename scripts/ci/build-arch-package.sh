#!/usr/bin/env bash
# Exercise the real Arch package() recipe using the portable build's ELF files.
set -euo pipefail
[[ -f /.dockerenv ]]
VERSION=$(sed -n 's/^VERSION = //p' /source/Makefile)
pacman -Syu --noconfirm --needed base-devel
useradd --create-home packager
BUILD=$(mktemp -d)
chmod 755 "$BUILD"
cp /source/packaging/arch/{PKGBUILD,uscreen.install} "$BUILD/"
mkdir -p "$BUILD/src"
tar -xf "/artifacts/uscreen-$VERSION-linux-x86_64.tar.gz" -C "$BUILD/src"
mv "$BUILD/src/uscreen-$VERSION" "$BUILD/src/UScreen-$VERSION"
D="$BUILD/src/UScreen-$VERSION"
mkdir -p "$D/target/release" "$D/host/evdi" "$BUILD/src/evdi-1.15.0/library"
cp "$D/bin/uscreen" "$D/bin/uscreen-gui" "$D/target/release/"
cp "$D/bin/evdi_helper" "$D/host/evdi/"
cp "$D/bin/libevdi.so.1.15.0" "$BUILD/src/evdi-1.15.0/library/"
# Include the original distribution-docs manifest and copier for package().
cp -a /source/scripts /source/packaging "$D/"
chown -R packager:packager "$BUILD"
runuser -u packager -- bash -c '
    set -euo pipefail
    cd "$1"
    source PKGBUILD
    srcdir="$PWD/src"; pkgdir="$PWD/pkg/uscreen"
    mkdir -p "$pkgdir"
    cd "$srcdir"
    package
    cd "$1"
    makepkg --repackage --nodeps --nosign --force
' uscreen-arch "$BUILD"
cp "$BUILD"/uscreen-[0-9]*.pkg.tar.zst /artifacts/
