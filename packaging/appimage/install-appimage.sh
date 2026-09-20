#!/usr/bin/env bash
# Explicit user registration; never remove a system package or activate EVDI.
set -euo pipefail
[[ $# -eq 0 ]] || { echo 'Usage: UScreen.AppImage --install-user' >&2; exit 2; }
umask 077
[[ ${HOME:-} = /* ]] || { echo "HOME must be absolute." >&2; exit 2; }

absolute_base() {
    case "$1" in /*) printf '%s' "$1" ;; *) printf '%s' "$2" ;; esac
}
DATA_BASE=$(absolute_base "${XDG_DATA_HOME:-}" "$HOME/.local/share")
CONFIG_BASE=$(absolute_base "${XDG_CONFIG_HOME:-}" "$HOME/.config")
DEST="$DATA_BASE/uscreen/appimage"
TOOLS="$APPDIR/usr/share/uscreen"

check_stopped() {
    local state
    state=$("$APPDIR/usr/bin/uscreen" status)
    case "$state" in
        *'uscreen is not running'*) ;;
        *) echo 'Stop the running UScreen daemon before installing this AppImage.' >&2; exit 1 ;;
    esac
}

install_image() {
    local temporary
    temporary=$(mktemp "$DEST/.image.XXXXXXXX")
    if cp -- "$APPIMAGE" "$temporary" && chmod 755 "$temporary" && mv -f -- "$temporary" "$DEST/UScreen.AppImage"; then
        LAUNCHER="$DEST/UScreen.AppImage"
    else
        rm -f -- "$temporary"
        return 1
    fi
}

install_extracted() {
    local temporary previous="$DEST/UScreen.AppDir.previous"
    [[ ! -e "$previous" ]] || { echo 'Resolve the previous interrupted AppDir installation first.' >&2; return 1; }
    temporary=$(mktemp -d "$DEST/.appdir.XXXXXXXX")
    if ! cp -a -- "$APPDIR/." "$temporary/"; then rm -rf -- "$temporary"; return 1; fi
    if [[ -e "$DEST/UScreen.AppDir" ]]; then mv -- "$DEST/UScreen.AppDir" "$previous"; fi
    if mv -- "$temporary" "$DEST/UScreen.AppDir"; then
        rm -rf -- "$previous"
    else
        [[ ! -e "$previous" ]] || mv -- "$previous" "$DEST/UScreen.AppDir"
        rm -rf -- "$temporary"
        return 1
    fi
    LAUNCHER="$DEST/UScreen.AppDir/AppRun"
}

install_service() {
    local temporary marker
    mkdir -p -- "$CONFIG_BASE/systemd/user"
    temporary=$(mktemp "$CONFIG_BASE/systemd/user/.uscreen.XXXXXXXX")
    marker=$(printf '%s' "$LAUNCHER" | od -An -tx1 | tr -d ' \n')
    printf '# USCREEN_APPIMAGE_PATH_HEX=%s\n' "$marker" > "$temporary"
    bash "$TOOLS/write-systemd-service.sh" "$ENTRY" "$TOOLS/uscreen.service" >> "$temporary"
    mv -f -- "$temporary" "$CONFIG_BASE/systemd/user/uscreen.service"
    systemctl --user daemon-reload 2>/dev/null || true
}

install_desktop() {
    local temporary
    mkdir -p -- "$DATA_BASE/applications" "$DATA_BASE/icons/hicolor/scalable/apps" "$HOME/.local/bin"
    temporary=$(mktemp "$DATA_BASE/applications/.uscreen.XXXXXXXX")
    bash "$TOOLS/write-desktop-entry.sh" "$ENTRY" "$TOOLS/uscreen.desktop" --gui > "$temporary"
    mv -f -- "$temporary" "$DATA_BASE/applications/uscreen.desktop"
    cp -- "$APPDIR/usr/share/icons/hicolor/scalable/apps/uscreen.svg" "$DATA_BASE/icons/hicolor/scalable/apps/"
    ln -sfnT -- "$ENTRY" "$HOME/.local/bin/uscreen"
    install_gui_entry
}

backup_gui_entry() {
    local entry="$1" backup
    [[ -e "$entry" || -L "$entry" ]] || return 0
    [[ -f "$entry" || -L "$entry" ]] || { echo 'GUI launcher destination is not a file or link.' >&2; return 1; }
    backup=$(mktemp -d "$DEST/gui-backup.XXXXXXXX")
    mv -T -- "$entry" "$backup/uscreen-gui"
    printf '%s' "$backup/uscreen-gui"
}

link_gui_entry() {
    local entry="$HOME/.local/bin/uscreen-gui" backup
    if [[ -L "$entry" ]] && [[ $(readlink -- "$entry") = "$DEST/uscreen-gui" ]]; then return 0; fi
    backup=$(backup_gui_entry "$entry") || return
    if ln -sT -- "$DEST/uscreen-gui" "$entry"; then return 0; fi
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
# USCREEN_APPIMAGE_GUI_WRAPPER=1
set -eu
launcher=$(readlink -f -- "$0")
exec "${launcher%/*}/gui-launcher" --gui "$@"
GUI_SCRIPT
    chmod 755 "$temporary"
    mv -f -- "$temporary" "$DEST/uscreen-gui"
    link_gui_entry
}

install_entry() {
    ENTRY=$LAUNCHER
    [[ $LAUNCHER = "$DEST/UScreen.AppImage" ]] || return 0
    local temporary
    temporary=$(mktemp "$DEST/.launcher.XXXXXXXX")
    cat > "$temporary" <<'ENTRY_SCRIPT'
#!/bin/sh
set -eu
launcher=$(readlink -f -- "$0")
export APPIMAGE_EXTRACT_AND_RUN=1
exec "${launcher%/*}/UScreen.AppImage" "$@"
ENTRY_SCRIPT
    chmod 755 "$temporary"
    mv -f -- "$temporary" "$DEST/uscreen"
    ENTRY="$DEST/uscreen"
}

update_autostart() {
    local entry="$CONFIG_BASE/autostart/uscreen.desktop" temporary
    if systemctl --user is-enabled uscreen.service >/dev/null 2>&1; then
        mkdir -p -- "$CONFIG_BASE/autostart"
        temporary=$(mktemp "$CONFIG_BASE/autostart/.uscreen.XXXXXXXX")
        cat "$TOOLS/uscreen-service-autostart.desktop" > "$temporary"
    else
        [[ -f "$entry" ]] || return 0
        temporary=$(mktemp "$CONFIG_BASE/autostart/.uscreen.XXXXXXXX")
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
printf 'Installed at %s\nAutostart preference is unchanged; enable it in UScreen settings.\n' "$LAUNCHER"
