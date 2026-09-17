#!/usr/bin/env bash
# UScreen installer — works from a source checkout or a release tarball.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"

GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }

is_prebuilt() { [ -f "$PROJECT_DIR/bin/uscreen" ]; }

has_evdi_library() {
    # Source linking needs the unversioned development link. A prebuilt helper
    # instead needs its runtime SONAME, supplied by a bundle or the system.
    local name=libevdi.so directory
    if is_prebuilt; then
        name=libevdi.so.1
        [ -f "$PROJECT_DIR/bin/$name" ] && return 0
    fi
    for directory in /usr/local/lib /usr/local/lib64 /usr/local/lib/* \
                     /usr/lib /usr/lib64 /usr/lib/* /lib /lib64 /lib/*; do
        [ -f "$directory/$name" ] && return 0
    done
    return 1
}

install_debian_evdi_library() {
    has_evdi_library && return 0
    local package=libevdi-dev
    if is_prebuilt; then package=libevdi1; fi
    sudo apt-get install -y "$package" || \
        warn "$package did not install — supply libevdi separately (see docs/development.md)"
}

# Shared runtime package names for RPM distributions. Repository selection
# remains specific to each family; Enterprise Linux must never inherit Fedora's.
RPM_RUNTIME_DEPS=(ffmpeg android-tools libX11 libX11-xcb libXcursor libXi libxkbcommon-x11)

is_ostree_booted() {
    command -v rpm-ostree &>/dev/null && [ -e /run/ostree-booted ]
}

install_rpm_layered_deps() {
    info "Immutable RPM system detected — layering packages (reboot needed afterwards)"
    sudo rpm-ostree install --idempotent --allow-inactive "${RPM_RUNTIME_DEPS[@]}" || \
        warn "Layering failed — check repositories and package names against your image"
}

install_rpm_runtime_deps() {
    sudo dnf install -y "$@" "${RPM_RUNTIME_DEPS[@]}" || \
        warn "Install runtime packages for this distribution manually: ${RPM_RUNTIME_DEPS[*]}"
}

check_rpm_evdi_module() {
    if ! modinfo evdi >/dev/null 2>&1; then
        warn "No evdi module found for the running kernel. Check your image/vendor or build a compatible module; see docs/installation.md."
    fi
}

enable_fedora_rpmfusion() {
    local release
    release=$(rpm -E %fedora)
    case "$release" in
        ''|*[!0-9]*) warn "Cannot identify the Fedora release. Configure repositories manually for your release; see docs/installation.md."; return ;;
    esac
    info "Enabling RPM Fusion for Fedora $release (ffmpeg is unavailable in configured repositories)"
    sudo dnf install -y \
        "https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$release.noarch.rpm" || \
        warn "Could not enable RPM Fusion"
}

install_fedora_deps() {
    if is_ostree_booted; then
        install_rpm_layered_deps
    else
        if ! dnf -q info ffmpeg >/dev/null 2>&1; then enable_fedora_rpmfusion; fi
        # Permit replacing conflicting ffmpeg-free packages on Fedora.
        install_rpm_runtime_deps --allowerasing
    fi
    check_rpm_evdi_module
}

install_enterprise_deps() {
    info "Enterprise Linux ${VERSION_ID:-unknown version}: using configured repositories"
    warn "If packages are unavailable, configure repositories for your Enterprise Linux release (see docs/installation.md), then retry."
    if is_ostree_booted; then
        install_rpm_layered_deps
    else
        install_rpm_runtime_deps
    fi
    check_rpm_evdi_module
}

install_debian_deps() {
    local gui_deps=(libx11-6 libx11-xcb1 libxcursor1 libxi6 libxkbcommon-x11-0)
    sudo apt-get update
    # Independent transactions: missing libevdi or a failed DKMS build must
    # not prevent runtime tools from installing, including in prebuilt bundles.
    sudo apt-get install -y ffmpeg adb "${gui_deps[@]}" || \
    sudo apt-get install -y ffmpeg android-tools-adb "${gui_deps[@]}" || \
        warn "Check the package names for your release"
    install_debian_evdi_library
    sudo apt-get install -y evdi-dkms || \
        warn "evdi-dkms did not install — you may need linux-headers-$(uname -r)"
}

install_arch_deps() {
    sudo pacman -S --needed --noconfirm ffmpeg android-tools libx11 libxcursor libxi libxkbcommon-x11
    # evdi is not in the official repositories on Arch — it only
    # exists in the AUR, so asking pacman for it can never succeed.
    if pacman -Qq evdi-dkms >/dev/null 2>&1 || pacman -Qq evdi >/dev/null 2>&1; then
        info "evdi already installed"
    elif command -v yay >/dev/null 2>&1; then
        yay -S --needed --noconfirm evdi-dkms
    elif command -v paru >/dev/null 2>&1; then
        paru -S --needed --noconfirm evdi-dkms
    else
        warn "evdi lives in the AUR. Install it with an AUR helper, e.g."
        warn "    yay -S evdi-dkms"
        warn "then run this script again."
    fi
}

install_suse_deps() {
    local gui_deps=(libX11-6 libX11-xcb1 libXcursor1 libXi6 libxkbcommon-x11-0)
    # The one distribution that has all of it in the default repos.
    # Split in two: the evdi kernel module package is tied to the
    # running kernel's ABI, and when that does not resolve it should
    # not take ffmpeg and adb down with it.
    sudo zypper --non-interactive install --no-recommends \
        ffmpeg android-tools "${gui_deps[@]}" || \
        warn "Install ffmpeg, android-tools and the GUI libraries (${gui_deps[*]}) manually"
    # libevdi1 requires evdi-kmp, so the library and the kernel module
    # stand or fall together here — nothing to be gained by splitting
    # them further.
    sudo zypper --non-interactive install --no-recommends evdi libevdi1 || \
        warn "evdi did not install — usually a kernel/module version mismatch"
}

install_distro_deps() {
    # Match whole ID/ID_LIKE tokens. EL derivatives can also list Fedora;
    # their more specific family takes precedence.
    case " $1 " in
        *' rhel '*|*' centos '*|*' rocky '*|*' almalinux '*) install_enterprise_deps ;;
        *' fedora '*) install_fedora_deps ;;
        *' debian '*|*' ubuntu '*) install_debian_deps ;;
        *' arch '*|*' manjaro '*|*' endeavouros '*|*' cachyos '*) install_arch_deps ;;
        *' suse '*|*' opensuse '*|*' opensuse-leap '*|*' opensuse-tumbleweed '*) install_suse_deps ;;
        *) warn "Unknown distro. Install manually: ffmpeg, adb (android-tools), evdi + libevdi, and GUI libraries: X11, X11-xcb, Xcursor, Xi, xkbcommon-x11" ;;
    esac
}

install_deps() {
    . /etc/os-release 2>/dev/null || true
    local id="${ID:-unknown}"
    local like="${ID_LIKE:-}"

    install_distro_deps "$id $like"
}

# Say what is missing before the build fails on it in a less obvious way.
check_deps() {
    local missing=0
    command -v ffmpeg >/dev/null 2>&1 || { warn "ffmpeg not found"; missing=1; }
    command -v adb    >/dev/null 2>&1 || { warn "adb not found"; missing=1; }
    has_evdi_library || \
        { warn "libevdi not found for the selected source/runtime route (see docs/development.md)"; missing=1; }
    [ "$missing" = 0 ] && info "Dependencies look complete"
    return 0
}

# ~/.local/bin is only added to PATH at login on most distributions, and only
# if it already exists. Installing into a directory we just created therefore
# gives "uscreen: command not found" straight after a successful install.
check_path() {
    case ":${PATH}:" in
        *":${BIN_DIR}:"*) return 0 ;;
    esac
    warn "$BIN_DIR is not in your PATH. Add it with:"
    warn "    echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc"
    warn "or log out and back in — most shells pick it up once the directory exists."
}

build_if_needed() {
    # Release tarballs ship prebuilt binaries next to this script's parent
    if is_prebuilt; then
        return
    fi
    # Cargo tracks source/dependency changes; make also rebuilds the helper.
    # A daemon binary alone says nothing about the other outputs or freshness.
    info "Building from source (needs rust + gcc)..."
    make -C "$PROJECT_DIR" build
}

stage_install_binaries() {
    local src_bin="$1" staged="$2" helper="$1/evdi_helper"
    cp "$src_bin/uscreen" "$staged/uscreen" || return
    if [ -f "$src_bin/uscreen-gui" ]; then
        cp "$src_bin/uscreen-gui" "$staged/uscreen-gui" || return
    else
        warn "uscreen-gui not found, skipping"
    fi
    if [ ! -f "$helper" ]; then
        helper="$PROJECT_DIR/host/evdi/evdi_helper"
    fi
    cp "$helper" "$staged/evdi_helper" || return
    # Only program files are staged so far, including the optional GUI.
    chmod +x "$staged/"* || return
    if [ -f "$src_bin/libevdi.so.1.15.0" ]; then
        cp -P "$src_bin"/libevdi.so.1* "$staged/" || return
    fi
}

install_binaries() {
    local src_bin staged
    if is_prebuilt; then
        src_bin="$PROJECT_DIR/bin"
    else
        # Make builds into this directory explicitly, regardless of Cargo's
        # environment/config target-dir. Keep every source consumer aligned.
        src_bin="$PROJECT_DIR/target/release"
    fi

    staged=$(mktemp -d "$BIN_DIR/.uscreen-install.XXXXXXXX") || return
    # Finish every copy before replacing installed names. Atomic replacement
    # also preserves the inodes mapped by running executables and libraries.
    if stage_install_binaries "$src_bin" "$staged" && mv -f "$staged/"* "$BIN_DIR/"; then
        rmdir "$staged"
        info "Binaries installed to $BIN_DIR"
    else
        rm -rf "$staged"
        return 1
    fi
}

install_desktop_entry() {
    # Absolute path: the app menu does not necessarily have ~/.local/bin on
    # its PATH, so a bare "uscreen-gui" can be a menu entry that does nothing.
    bash "$SCRIPT_DIR/write-desktop-entry.sh" "$BIN_DIR/uscreen-gui" "$SCRIPT_DIR/uscreen.desktop" > "$APP_DIR/uscreen.desktop" \
        && info "Desktop entry installed (UScreen in the app menu)"
    return 0
}

install_icons() {
    # The menu entry and the tray look the icon up by name in the hicolor
    # theme; without this they fall back to a generic or blank picture.
    local ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
    if [ -f "$PROJECT_DIR/packaging/icons/uscreen.svg" ]; then
        mkdir -p "$ICON_DIR"
        cp "$PROJECT_DIR/packaging/icons/uscreen.svg" "$PROJECT_DIR/packaging/icons/uscreen-pen.svg" "$ICON_DIR/"
        # KDE keys its icon cache on the theme directory's mtime.
        touch "${ICON_DIR%/scalable/apps}" 2>/dev/null || true
        command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "${ICON_DIR%/scalable/apps}" 2>/dev/null || true
        command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true
    fi
}

install_user_service() {
    mkdir -p "${HOME}/.config/systemd/user"
    cp "$SCRIPT_DIR/uscreen.service" "${HOME}/.config/systemd/user/" 2>/dev/null || true
    systemctl --user daemon-reload 2>/dev/null || true
    # Enabled, not started: the system setup below (evdi module, udev rule)
    # has not run yet, so a start here would fail on a fresh machine. The
    # closing message says how to start it now; it starts by itself from the
    # next login on.
    systemctl --user enable uscreen.service 2>/dev/null || true
}

install_files() {
    mkdir -p "$BIN_DIR" "$APP_DIR"
    install_binaries
    install_desktop_entry
    install_icons
    install_user_service
}

configure_boot_modules() {
    # Neither directory is guaranteed to exist on a minimal install, and with
    # set -e a missing one used to kill the whole script here, silently, with
    # the binaries already copied and the udev rule not yet installed.
    sudo mkdir -p /etc/modprobe.d /etc/modules-load.d
    echo "options evdi initial_device_count=2" | sudo tee /etc/modprobe.d/uscreen-evdi.conf >/dev/null \
        || warn "Could not write /etc/modprobe.d/uscreen-evdi.conf"
    printf "evdi\nuinput\n" | sudo tee /etc/modules-load.d/uscreen.conf >/dev/null \
        || warn "Could not write /etc/modules-load.d/uscreen.conf"
}

configure_uinput() {
    sudo modprobe uinput 2>/dev/null || true
    # /dev/uinput is root-only on a stock system. Bazzite ships a rule that
    # opens it to the seat user; everyone else needs this one.
    if [ ! -e /etc/udev/rules.d/60-uscreen-uinput.rules ] && [ ! -e /usr/lib/udev/rules.d/60-uscreen-uinput.rules ]; then
        if [ -f "$PROJECT_DIR/packaging/60-uscreen-uinput.rules" ]; then
            sudo install -Dm644 "$PROJECT_DIR/packaging/60-uscreen-uinput.rules" /etc/udev/rules.d/60-uscreen-uinput.rules
            sudo udevadm control --reload 2>/dev/null || true
            sudo udevadm trigger --name-match=uinput 2>/dev/null || true
        else
            warn "packaging/60-uscreen-uinput.rules not found — /dev/uinput may stay root-only"
        fi
    fi
}

activate_evdi() {
    sudo sh "$SCRIPT_DIR/setup-evdi.sh" 2 || warn "EVDI setup deferred; existing devices were preserved"
}

system_setup() {
    info "System setup (needs sudo): EVDI device at every boot"
    configure_boot_modules
    configure_uinput
    activate_evdi
}

main() {
    # Make has already built the source tree and owns the remaining setup.
    if [[ ${1:-} == --binaries-only ]]; then
        BIN_DIR="$2"
        mkdir -p "$BIN_DIR"
        install_binaries
        return
    fi
    echo "================================================"
    echo "  UScreen installer"
    echo "================================================"
    install_deps
    check_deps
    build_if_needed
    install_files
    system_setup
    echo ""
    info "Done! Launch 'UScreen' from your app menu (or run: uscreen-gui)"
    info "The daemon starts with your next login; to start it now: systemctl --user start uscreen"
    info "Install the APK on your tablet, enable USB debugging, plug in — that's it."
    check_path
}

main "$@"
