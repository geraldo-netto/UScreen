# Security

## What UScreen does on your machine

- The daemon listens on **loopback only** (127.0.0.1): video on port 8890,
  input on 8891, plus two ports per additional tablet slot. Nothing is exposed
  on the network. The tablet reaches these ports through `adb reverse`.
- Every daemon run generates a **random 256-bit session token**. It is handed
  to the app over adb (on stdin, never on a command line) and a client that
  does not present it first receives no video and cannot inject input. This
  protects against other local accounts and other apps on the tablet.
  Processes running as your own Linux user can read your session token.
  Android accepts token updates through a separate activity guarded by the
  system DUMP permission, available to adb shell and privileged apps; the
  normal launcher ignores token extras.
- The capture FIFO and the token live in `$XDG_RUNTIME_DIR/uscreen/`, a
  per-user directory with mode 0700; both files are 0600.
- **Screen and input data travel only between your computer and tablet,**
  over USB or your own network if you chose Wi-Fi.
  There is no account, no telemetry and no cloud.
- The only automatic internet connection is an optional **update check**: one HTTPS
  request to `api.github.com` (daemon: once a day; app and GUI: when opened)
  that reads the latest release tag. Off with `check_updates = false` for the
  daemon and GUI, and with the *Check for newer releases* switch in the app's
  settings sheet.
  Nothing is ever downloaded or installed automatically.

## What the installer and packages change on the system

All of it is needed to create a virtual display and inject input as an
unprivileged user; nothing else is touched.

| change | why |
| --- | --- |
| `/etc/modprobe.d/uscreen-evdi.conf` (script) or `/usr/lib/modprobe.d/uscreen-evdi.conf` (package) — `options evdi initial_device_count=2` | two EVDI virtual-display devices exist from boot; creating them later needs root |
| `/etc/modules-load.d/uscreen.conf` (script) or `/usr/lib/modules-load.d/uscreen.conf` (package) — `evdi`, `uinput` | load both modules at boot |
| `/usr/lib/udev/rules.d/60-uscreen-uinput.rules` (or `/etc/udev/rules.d/`) | opens `/dev/uinput` to the logged-in seat user via `uaccess`, the same mechanism the desktop uses for keyboards |
| a systemd **user** unit `uscreen.service` | optional autostart with your session; never a system service, never root |

The daemon itself never runs as root and needs no capabilities.

## How to uninstall completely

```bash
systemctl --user disable --now uscreen 2>/dev/null
# Also stop a daemon launched from the terminal or the GUI without systemd.
uscreen stop
# package installs:
sudo apt remove uscreen      # or: sudo dnf remove uscreen / sudo zypper rm uscreen / sudo pacman -R uscreen
# script installs:
rm -f ~/.local/bin/uscreen ~/.local/bin/uscreen-gui ~/.local/bin/evdi_helper ~/.local/bin/libevdi.so.1*
rm -f ~/.config/systemd/user/uscreen.service ~/.local/share/applications/uscreen.desktop
rm -f ~/.local/share/icons/hicolor/scalable/apps/uscreen.svg ~/.local/share/icons/hicolor/scalable/apps/uscreen-pen.svg
# script-created system changes (package removal handles /usr/lib files):
sudo rm -f /etc/modprobe.d/uscreen-evdi.conf /etc/modules-load.d/uscreen.conf /etc/udev/rules.d/60-uscreen-uinput.rules
# your settings and logs:
rm -rf ~/.config/uscreen ~/.local/share/uscreen ~/.cache/uscreen
```

On the tablet, uninstall the app like any other.

## Release integrity

Every release includes `SHA256SUMS` for all files. The Android APK is signed
with one key across all releases (SHA-256 of the certificate:
`ee813fed1a89f1b9105af6afb27542b1b945bb2c1882d2a05ec2a9ab5fa8a759`), so an
update that does not install over the previous version was not built by this
project.
