#!/bin/sh
# Run as root. Loading is idempotent; never unload a live DRM device.
# Existing devices may belong to another display client and must survive upgrades.
evdi_setup_deferred() {
    echo "uscreen: $1; reboot after checking the evdi module and boot configuration, then run uscreen doctor" >&2
    return 1
}

evdi_device_count() {
    uscreen_count=$(cat /sys/devices/evdi/count 2>/dev/null) || return 1
    case "$uscreen_count" in ''|*[!0-9]*) return 1 ;; esac
    printf '%s\n' "$uscreen_count"
}

evdi_add_capacity() {
    printf '%s\n' "$1" | tee /sys/devices/evdi/add >/dev/null 2>&1 \
        || evdi_setup_deferred "could not add the missing EVDI devices"
}

evdi_setup() {
    uscreen_wanted=$1
    modprobe evdi 2>/dev/null || {
        evdi_setup_deferred "evdi module is unavailable"
        return 1
    }
    uscreen_existing=$(evdi_device_count) || {
        evdi_setup_deferred "cannot read the EVDI device count"
        return 1
    }
    if [ "$uscreen_existing" -lt "$uscreen_wanted" ]; then
        evdi_add_capacity "$((uscreen_wanted - uscreen_existing))" || return 1
        uscreen_existing=$(evdi_device_count) || {
            evdi_setup_deferred "cannot confirm the new EVDI capacity"
            return 1
        }
    fi
    if [ "$uscreen_existing" -lt "$uscreen_wanted" ]; then
        evdi_setup_deferred "required EVDI capacity is not available"
        return 1
    fi
    echo "uscreen: EVDI devices ready (count=$uscreen_existing); existing devices preserved"
}

case "${1:-2}" in
    1|2|3|4) evdi_setup "${1:-2}" ;;
    *) echo "Usage: setup-evdi.sh [1-4]" >&2; exit 2 ;;
esac
