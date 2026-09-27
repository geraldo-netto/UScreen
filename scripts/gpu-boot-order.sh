#!/usr/bin/env bash
# Copyright (c) 2026 Geraldo Netto
# Linux adapter: configure the next EVDI load without loading/unloading a module.
set -euo pipefail
warn() { printf 'blent: %s\n' "$1" >&2; }

gpu_boot_module() {
    local module
    # Preserve trailing newlines in a name so validation rejects them, instead
    # of command substitution silently turning malformed metadata into a token.
    # Resolve the DRM provider's parent through the class symlink. The card's
    # device link can instead point at a transport, e.g. virtio_pci.
    module=$(readlink -e "$1/../../driver/module" && printf '.') || return 0
    module=${module%$'\n.'}
    module=${module##*/}
    case "$module" in
        evdi|simpledrm) return 0 ;;
        nvidia) module=nvidia_drm ;;
        virtio_pci) module=virtio_gpu ;;
    esac
    [[ $module =~ ^[a-zA-Z0-9_][a-zA-Z0-9_-]{0,63}$ ]] || return 1
    printf '%s\n' "$module"
}

gpu_boot_modules() {
    local card modules='' module
    for card in "$1"/card*; do
        [[ ${card##*/} =~ ^card[0-9]+$ ]] || continue
        module=$(gpu_boot_module "$card") || return 1
        if [ -n "$module" ]; then modules+="$module"$'\n'; fi
    done
    if [ -n "$modules" ]; then printf '%s' "$modules" | LC_ALL=C sort -u; fi
}

write_gpu_boot_order() (
    local modules="$1" config="$2" temporary
    if [ -e "$config" ] || [ -L "$config" ]; then
        warn "Existing $config preserved; review GPU ordering after hardware changes."
        return 0
    fi
    temporary=$(mktemp "${config%/*}/.blent-gpu-order.XXXXXXXX") || return
    trap 'rm -f -- "$temporary"' EXIT
    printf 'softdep evdi pre: %s\n' "${modules//$'\n'/ }" > "$temporary" || return
    chmod 644 "$temporary" || return
    # Publish atomically without replacing a concurrent file or symlink.
    ln -T -- "$temporary" "$config"
)

configure_gpu_boot_order() {
    local modules
    modules=$(gpu_boot_modules "$1") || {
        warn "Invalid GPU module metadata; boot ordering unchanged. See docs/installation.md."
        return 1
    }
    if [ -z "$modules" ]; then
        warn "No loadable physical GPU found; check GPU readiness before Xorg startup. See docs/installation.md."
        return 0
    fi
    write_gpu_boot_order "$modules" "$2/blent-gpu-order.conf"
}

# Explicit roots support isolated fixtures and administrator-managed image staging.
[[ $# -le 2 ]] || { warn 'Usage: gpu-boot-order.sh [DRM_SYSFS CONFIG_DIRECTORY]'; exit 2; }
mkdir -p -- "${2:-/etc/modprobe.d}"
configure_gpu_boot_order "${1:-/sys/class/drm}" "${2:-/etc/modprobe.d}"
