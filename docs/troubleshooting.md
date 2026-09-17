# Troubleshooting

Start with `uscreen doctor`. It checks configuration, tools, devices, adb and
selected desktop/decoder state; its suggestions are diagnostics, not proof
that every desktop or encoding combination is supported. Include the exact
fork commit as well as host/app version numbers in a report.

## "Failed to start helper" / "evdi-helper exited prematurely"

Read the preceding error. Possible causes include a missing/incompatible
EVDI module or userspace library, permissions, a busy card, invalid EDID or a
missing helper executable. Check `modinfo evdi`, `cat /sys/devices/evdi/count`
and `ldd` on the installed `evdi_helper`. A missing sysfs path differs from
a loaded module reporting zero devices.

If the module is loaded and the count is zero, these commands request one
device now and configure two at the next module load:

```bash
echo 1 | sudo tee /sys/devices/evdi/add
echo 'options evdi initial_device_count=2' | sudo tee /etc/modprobe.d/uscreen-evdi.conf
```

Use GUI system setup to add missing capacity for a larger tablet count.
Reboot if a changed boot setting needs to take effect. **Do not unload EVDI
from a running display session**: it can disrupt the display server. Installer
reload behavior remains tracked as T269 in [TODO.md](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/TODO.md).

## "Failed to open /dev/uinput"

Check whether the module is loaded and the current desktop seat has access.
From a source checkout, when the UScreen udev rule is missing:

```bash
sudo modprobe uinput
sudo install -Dm644 packaging/60-uscreen-uinput.rules /etc/udev/rules.d/60-uscreen-uinput.rules
sudo udevadm control --reload
sudo udevadm trigger --name-match=uinput
```

Run UScreen as the logged-in desktop user; do not use a root daemon as a
permission workaround.

## The app keeps opening and closing / "did not authenticate"

Check that the app and host came from the same checkout and that adb still
shows the tablet as authorized. A stale token, failed token delivery or
mismatched builds can cause this; an older app is not the only explanation.
Keep `require_token = true` (the disabled setting has a known client mismatch,
T267). Delivery retries back off, potentially to ten minutes; reconnecting the
cable triggers a fresh attempt. Do not include session tokens in public logs.

## Black screen on the tablet

1. Check `uscreen status` and `adb devices`.
2. Check `adb -s TABLET_ID reverse --list` for that tablet's assigned ports
   (8890/8891 by default for the first slot).
3. For a service launch, inspect `journalctl --user -u uscreen -n 200`.
   Direct GUI launches log to `~/.local/share/uscreen/daemon.log`.
4. Check the selected encoder and desktop Display settings. The virtual
   output may be disabled, or FFmpeg may lack the selected encoder.

For a foreground debug run, first stop the service with
`systemctl --user stop uscreen` (or stop an unmanaged daemon with
`uscreen stop`), then run `RUST_LOG=uscreen=debug uscreen start`. This can
attach an EVDI display; it is not a read-only diagnostic. Do not deliberately
repeat a session-crashing attachment on your working desktop; see below.

## Touch or pen land on the wrong screen

KDE Wayland mapping uses KWin D-Bus. X11 mapping uses `xinput` and `xrandr`;
install both. Mapping runs on attachment and mode changes, and graphics-tablet
mode targets a physical screen. On modern KDE it can ignore the primary
output priority (T299). Check the resulting mapping after mode changes. Other Wayland desktops need
manual mapping where the compositor supports it. `doctor` can misdiagnose a
non-KDE Wayland session as missing KWin (T234).

For an X11 session, inspect `xrandr --listproviders` and `xrandr --query`.
If the EVDI provider is not linked, the general command is
`xrandr --setprovideroutputsource EVDI_PROVIDER SOURCE_PROVIDER`, replacing
both names/IDs with the appropriate providers from that output. Do not assume
the source is provider 0. Display-server/driver support is required; this
command changes the live display configuration.

Automatic KScreen placement runs only when `XDG_SESSION_TYPE=wayland` and
`XDG_CURRENT_DESKTOP` identifies KDE (including colon-separated desktop names).
X11, other Wayland desktops and unidentified sessions skip those commands and
their retry delay; configure placement through the desktop's display settings.

## Cinnamon or Xorg restarts when connecting

The [2026-09-17 incident report](reviews/2026-09-17-cinnamon-restart.md)
records an Xorg crash during an EVDI attachment. The root cause and mitigation
remain unverified (T222). A later startup-geometry fix does not establish that
the crash is fixed. Preserve the journal and Xorg/coredump evidence; further
reproduction needs an isolated session.

## The on-screen keyboard pops up or remains disabled

UScreen suppresses KDE's virtual keyboard while its touch devices exist and
attempts to restore the saved setting afterward. An invalid/missing recovery
value can prevent restoration (T334). If it remains disabled, restore the
desired setting in System Settings → Virtual Keyboard; simply restarting the
daemon is not a guaranteed recovery.

## Wi-Fi is stuttery

The historical locked-radio test had a median close to USB, but much longer
tail delays. Your network may differ. See [benchmarks.md](benchmarks.md).
Use USB when those delays are disruptive. `uscreen wifi --off` does not close
the tablet's adb network listener; see [SECURITY.md](../SECURITY.md).

## A required GLIBC version is not found

The binary or one of its libraries was built against a newer glibc than the
runtime provides. The portable workflow targets glibc 2.36; local builds may
need newer versions regardless of the displayed UScreen version. Use a build
matching your distribution, or rebuild with the documented portable toolchain.
Do not replace the system glibc to satisfy an application binary.

## A busy EVDI card is selected despite another free card

Automatic sessions lease free cards rather than assigning them by enumeration
order. Restarting capture prefers its previous card if still free, otherwise
it searches the current card set. Explicit helper `--card` pins remain strict.
If every card is connected or leased, the helper tries to add a device; check
the resulting error and EVDI permissions if capture keeps retrying. Include
card/connector state in reports without removing another application's display.

## Getting more help

Open a [fork issue](https://github.com/geraldo-netto/UScreen/issues) with the
build commit, `uscreen doctor` output and relevant log excerpt. Remove tokens,
device serials and other personal information before posting.
