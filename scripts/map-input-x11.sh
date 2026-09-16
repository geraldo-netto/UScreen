#!/usr/bin/env bash
# Map the UScreen touch/pen devices onto the virtual (EVDI) output on X11.
#
# On KDE Plasma (Wayland) the daemon does this itself over KWin's D-Bus
# interface. On X11 desktops (Cinnamon, XFCE, MATE, GNOME on Xorg, ...)
# nothing does, so an absolute device spans the whole desktop and taps land
# on the wrong screen. `xinput map-to-output` fixes that per device. X11 does
# not persist it, and the devices only exist while a tablet is attached: run
# again after the tablet reconnects or the daemon restarts.
#
# Usage: scripts/map-input-x11.sh [OUTPUT_NAME]
#   OUTPUT_NAME defaults to the Xorg output of the first connected EVDI
#   connector. With max_tablets > 1 every UScreen device is mapped onto that
#   one output; name the output and map by hand for the others.
set -euo pipefail

out="${1:-}"
if [ -z "$out" ]; then
  # Find the EVDI connectors through sysfs, as the daemon does. Matching on
  # the output name alone ("DVI-I-*") is not safe: a real DVI monitor on a
  # dock has exactly the same name, and mapping onto it would send every tap
  # to the user's physical screen.
  connectors=()
  for card in /sys/devices/platform/evdi.*/drm/card*; do
    [ -d "$card" ] || continue
    for conn in "$card"/card*-*; do
      [ -d "$conn" ] || continue
      [ "$(cat "$conn/status" 2>/dev/null || true)" = connected ] || continue
      name=${conn##*/}          # card2-DVI-I-1
      connectors+=("${name#card*-}")   # DVI-I-1
    done
  done
  # Xorg names an offloaded output after its connector plus a provider
  # suffix: DVI-I-1 becomes DVI-I-1-1.
  outputs=$(xrandr --query | awk '/ connected/{print $1}')
  for c in ${connectors[@]+"${connectors[@]}"}; do
    for o in $outputs; do
      if [ "$o" = "$c" ] || [[ "$o" == "$c"-* ]]; then
        out=$o
        break 2
      fi
    done
  done
fi
if [ -z "$out" ]; then
  echo "No connected EVDI output found in Xorg. Connected outputs:" >&2
  xrandr --query | grep ' connected' >&2 || true
  echo "If none is EVDI, the tablet is not a screen yet, or Xorg has not linked" >&2
  echo "the evdi provider: try 'xrandr --listproviders' and" >&2
  echo "'xrandr --setprovideroutputsource <evdi-index> 0'." >&2
  exit 1
fi

echo "Mapping UScreen input devices to output: $out"
mapped=0
# Device names carry a " 2", " 3" suffix when max_tablets > 1, and libinput
# may split the pen into "UScreen Pen Pen (0)" and "UScreen Pen Eraser (0)".
while IFS= read -r line; do
  [[ $line =~ (UScreen\ (Touch|Pen|Pointer)[^[:cntrl:]]*[^[:space:]]).*id=([0-9]+) ]] || continue
  name=${BASH_REMATCH[1]}
  id=${BASH_REMATCH[3]}
  if xinput map-to-output "$id" "$out"; then
    echo "  mapped '$name' (id $id)"
    mapped=$((mapped + 1))
  fi
done < <(xinput list --short | grep -E 'UScreen (Touch|Pen|Pointer)' || true)

if [ "$mapped" -eq 0 ]; then
  echo "No UScreen input devices found. They exist only while a tablet is" >&2
  echo "attached, and only for the kinds enabled in the settings panel" >&2
  echo "(input_touch / input_pen in config.toml)." >&2
  exit 1
fi
