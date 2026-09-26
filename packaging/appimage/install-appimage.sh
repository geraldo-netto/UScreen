#!/usr/bin/env bash
# Explicit user registration; never remove a system package or activate EVDI.
set -euo pipefail
[[ $# -eq 0 ]] || { echo 'Usage: Blent.AppImage --install-user' >&2; exit 2; }
umask 077
[[ ${HOME:-} = /* ]] || { echo "HOME must be absolute." >&2; exit 2; }

absolute_base() {
    case "$1" in /*) printf '%s' "$1" ;; *) printf '%s' "$2" ;; esac
}
DATA_BASE=$(absolute_base "${XDG_DATA_HOME:-}" "$HOME/.local/share")
CONFIG_BASE=$(absolute_base "${XDG_CONFIG_HOME:-}" "$HOME/.config")
DEST="$DATA_BASE/blent/appimage"
TOOLS="$APPDIR/usr/share/blent"

check_stopped() {
    local state
    state=$("$APPDIR/usr/bin/blent" status)
    case "$state" in
        *'blent is not running'*) ;;
        *) echo 'Stop the running Blent daemon before installing this AppImage.' >&2; exit 1 ;;
    esac
}

install_image() {
    local temporary
    temporary=$(mktemp "$DEST/.image.XXXXXXXX")
    if cp -- "$APPIMAGE" "$temporary" && chmod 755 "$temporary" && mv -f -- "$temporary" "$DEST/Blent.AppImage"; then
        LAUNCHER="$DEST/Blent.AppImage"
    else
        rm -f -- "$temporary"
        return 1
    fi
}

install_extracted() {
    local temporary previous="$DEST/Blent.AppDir.previous"
    [[ ! -e "$previous" ]] || { echo 'Resolve the previous interrupted AppDir installation first.' >&2; return 1; }
    temporary=$(mktemp -d "$DEST/.appdir.XXXXXXXX")
    if ! cp -a -- "$APPDIR/." "$temporary/"; then rm -rf -- "$temporary"; return 1; fi
    if [[ -e "$DEST/Blent.AppDir" ]]; then mv -- "$DEST/Blent.AppDir" "$previous"; fi
    if mv -- "$temporary" "$DEST/Blent.AppDir"; then
        rm -rf -- "$previous"
    else
        [[ ! -e "$previous" ]] || mv -- "$previous" "$DEST/Blent.AppDir"
        rm -rf -- "$temporary"
        return 1
    fi
    LAUNCHER="$DEST/Blent.AppDir/AppRun"
}

install_service() {
    local temporary marker
    mkdir -p -- "$CONFIG_BASE/systemd/user"
    temporary=$(mktemp "$CONFIG_BASE/systemd/user/.blent.XXXXXXXX")
    marker=$(printf '%s' "$LAUNCHER" | od -An -tx1 | tr -d ' \n')
    printf '# BLENT_APPIMAGE_PATH_HEX=%s\n' "$marker" > "$temporary"
    bash "$TOOLS/write-systemd-service.sh" "$ENTRY" "$TOOLS/blent.service" >> "$temporary"
    mv -f -- "$temporary" "$CONFIG_BASE/systemd/user/blent.service"
    systemctl --user daemon-reload 2>/dev/null || true
}

install_desktop() {
    local temporary
    mkdir -p -- "$DATA_BASE/applications" "$DATA_BASE/icons/hicolor/scalable/apps" "$HOME/.local/bin"
    temporary=$(mktemp "$DATA_BASE/applications/.blent.XXXXXXXX")
    bash "$TOOLS/write-desktop-entry.sh" "$ENTRY" "$TOOLS/blent.desktop" --gui > "$temporary"
    mv -f -- "$temporary" "$DATA_BASE/applications/blent.desktop"
    cp -- "$APPDIR/usr/share/icons/hicolor/scalable/apps/blent.svg" "$DATA_BASE/icons/hicolor/scalable/apps/"
    ln -sfnT -- "$ENTRY" "$HOME/.local/bin/blent"
    install_gui_entry
}

backup_gui_entry() {
    local entry="$1" backup
    [[ -e "$entry" || -L "$entry" ]] || return 0
    [[ -f "$entry" || -L "$entry" ]] || { echo 'GUI launcher destination is not a file or link.' >&2; return 1; }
    backup=$(mktemp -d "$DEST/gui-backup.XXXXXXXX")
    mv -T -- "$entry" "$backup/blent-gui"
    printf '%s' "$backup/blent-gui"
}

link_gui_entry() {
    local entry="$HOME/.local/bin/blent-gui" backup
    if [[ -L "$entry" ]] && [[ $(readlink -- "$entry") = "$DEST/blent-gui" ]]; then return 0; fi
    backup=$(backup_gui_entry "$entry") || return
    if ln -sT -- "$DEST/blent-gui" "$entry"; then return 0; fi
    if [[ -n "$backup" ]]; then mv -T -- "$backup" "$entry"; fi
    return 1
}

install_gui_entry() {
    local temporary
    # A sibling link avoids embedding user paths in executable shell text.
    ln -sfnT -- "$ENTRY" "$DEST/gui-launcher"
    temporary=$(mktemp "$DEST/.gui-launcher.XXXXXXXX")
    cat > "$temporary" <<'GUI_SCRIPT'
#!/bin/sh
# BLENT_APPIMAGE_GUI_WRAPPER=1
set -eu
launcher=$(readlink -f -- "$0")
exec "${launcher%/*}/gui-launcher" --gui "$@"
GUI_SCRIPT
    chmod 755 "$temporary"
    mv -f -- "$temporary" "$DEST/blent-gui"
    link_gui_entry
}

install_entry() {
    ENTRY=$LAUNCHER
    [[ $LAUNCHER = "$DEST/Blent.AppImage" ]] || return 0
    local temporary
    temporary=$(mktemp "$DEST/.launcher.XXXXXXXX")
    cat > "$temporary" <<'ENTRY_SCRIPT'
#!/bin/sh
set -eu
launcher=$(readlink -f -- "$0")
export APPIMAGE_EXTRACT_AND_RUN=1
exec "${launcher%/*}/Blent.AppImage" "$@"
ENTRY_SCRIPT
    chmod 755 "$temporary"
    mv -f -- "$temporary" "$DEST/blent"
    ENTRY="$DEST/blent"
}

update_autostart() {
    local entry="$CONFIG_BASE/autostart/blent.desktop" temporary
    if systemctl --user is-enabled blent.service >/dev/null 2>&1; then
        mkdir -p -- "$CONFIG_BASE/autostart"
        temporary=$(mktemp "$CONFIG_BASE/autostart/.blent.XXXXXXXX")
        cat "$TOOLS/blent-service-autostart.desktop" > "$temporary"
    else
        [[ -f "$entry" ]] || return 0
        temporary=$(mktemp "$CONFIG_BASE/autostart/.blent.XXXXXXXX")
        bash "$TOOLS/write-desktop-entry.sh" "$ENTRY" "$entry" start > "$temporary"
    fi
    mv -f -- "$temporary" "$entry"
}

check_destination() {
    local destination
    destination=$(realpath -m -- "$DEST")
    case "$destination/" in
        "$APPDIR/"*) echo 'Installation destination must be outside the source AppDir.' >&2; return 1 ;;
    esac
}

check_stopped
check_destination
mkdir -p -- "$DEST"
if [[ -n ${APPIMAGE:-} && -f $APPIMAGE ]]; then install_image; else install_extracted; fi
install_entry
install_service
install_desktop
update_autostart
printf 'Installed at %s\nAutostart preference is unchanged; enable it in Blent settings.\n' "$LAUNCHER"
