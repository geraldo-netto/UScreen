#!/usr/bin/env bash
# Run only in a clean disposable container: no display server or host devices.
set -euo pipefail
[[ -f /.dockerenv ]]
trap 'echo "Package smoke failed at line $LINENO" >&2' ERR
case "$1" in
  debian)
    apt-get update
    apt-get install -y --no-install-recommends /artifacts/*.deb
    ;;
  fedora)
    dnf install -y https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-44.noarch.rpm
    dnf install -y --setopt=tsflags= --setopt=install_weak_deps=False /artifacts/*.rpm
    ;;
  opensuse)
    sed -i "s/^rpm.install.excludedocs *=.*/rpm.install.excludedocs = no/" /etc/zypp/zypp.conf
    zypper --non-interactive --gpg-auto-import-keys refresh
    zypper --non-interactive install --no-recommends --allow-unsigned-rpm /artifacts/*.rpm
    ;;
  arch)
    # Minimal images omit docs by default; test the package's full inventory.
    sed -i "\|^NoExtract.*usr/share/doc|d" /etc/pacman.conf
    pacman -Syu --noconfirm
    # A container shares the host kernel: exercise userspace dependencies only.
    pacman -U --noconfirm --assume-installed evdi-dkms /artifacts/uscreen-[0-9]*.pkg.tar.zst
    ;;
  *) echo "Unknown distro: $1" >&2; exit 2 ;;
esac

# Direct control must work with neither a systemd PID 1 nor a user bus.
[[ $(cat /proc/1/comm) != systemd ]]
unset DBUS_SESSION_BUS_ADDRESS
uscreen --version
uscreen status
uscreen stop
command -v ffmpeg
command -v adb
HELPER=/usr/lib/uscreen/evdi_helper
if [[ -f /usr/lib64/uscreen/evdi_helper ]]; then HELPER=/usr/lib64/uscreen/evdi_helper; fi
for binary in /usr/bin/uscreen /usr/bin/uscreen-gui "$HELPER"; do
    dependencies=$(ldd "$binary")
    printf '%s\n' "$dependencies"
    if grep -q 'not found' <<< "$dependencies"; then exit 1; fi
done
# The system's libevdi must not satisfy this check accidentally.
loaded_evdi=$(ldd "$HELPER" | awk '/libevdi.so.1 =>/ { print $3 }')
[[ $(readlink -f "$loaded_evdi") == $(readlink -f "$(dirname "$HELPER")/libevdi.so.1") ]]
if "$HELPER" > /tmp/helper-usage 2>&1; then helper_status=0; else helper_status=$?; fi
[[ $helper_status == 1 ]]
grep -q 'Usage:' /tmp/helper-usage
for file in LICENSE THIRD_PARTY_LICENSES.md licenses/libevdi-LGPL-2.1.txt README.md; do
    test -s "/usr/share/doc/uscreen/$file"
done

/source/scripts/ci/direct-control-smoke.sh

# Install only the test display server/tools here. GUI libraries must come
# from the package's runtime dependencies, not from the smoke environment.
case "$1" in
    debian) apt-get install -y --no-install-recommends xvfb x11-utils ;;
    fedora) dnf install -y --setopt=install_weak_deps=False xorg-x11-server-Xvfb xwininfo ;;
    opensuse) zypper --non-interactive install --no-recommends xorg-x11-server-Xvfb xwininfo ;;
    arch) pacman -S --noconfirm --needed xorg-server-xvfb xorg-xwininfo ;;
esac
timeout 20 /source/scripts/ci/gui-smoke.sh
