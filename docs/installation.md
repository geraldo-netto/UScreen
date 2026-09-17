# Installing UScreen for Linux

Build the Linux daemon/GUI/helper and Android app from the same checkout.
As of 2026-09-17 this fork has no published releases; start with
[building from source](development.md#building-from-source). Upstream releases
do not contain the unreleased fork changes. The instructions below also cover
artifacts produced locally or available in a future [fork release](https://github.com/geraldo-netto/UScreen/releases).

Before attaching a display, check [current limitations](compatibility.md#current-fork-limitations).
A Cinnamon/Xorg session crash during EVDI attachment remains unresolved (T222).
The full installer and native package hooks also attempt an EVDI module reload
(T269); installing/upgrading them during a live display session can disrupt it.
Schedule that setup outside an active EVDI session. This is separate from the
add-only setup in the GUI and `make setup-system`.

## Linux artifacts and prerequisites

The portable packaging workflow targets **Linux x86-64, glibc 2.36 or newer**.
This is an ABI baseline, not a guarantee that every distribution, GPU or
compositor works. Local builds can require a newer glibc. Runtime needs adb,
FFmpeg with the chosen encoder, compatible EVDI kernel support, libdrm, GUI
libraries and access to `/dev/uinput`. Container tests cannot validate kernel
attachment or tablet operation.

| Artifact | Installation |
| --- | --- |
| `uscreen_<ver>_amd64.deb` | Debian/Ubuntu family: `sudo apt install ./uscreen_*.deb` |
| `uscreen-<ver>-1.x86_64.rpm` | openSUSE: `sudo zypper install ./uscreen-*.rpm`; Fedora: supply the required FFmpeg package (the build workflow uses RPM Fusion), then `sudo dnf install --allowerasing ./uscreen-*.rpm` |
| `uscreen-<ver>-PKGBUILD.tar.gz` | Arch family: install AUR `evdi-dkms` first, extract the recipe and run `makepkg -si` |
| `uscreen-<ver>-linux-x86_64.tar.gz` | Other compatible Linux setups: extract and inspect/run `./scripts/install.sh`; unsupported package managers need manual dependency installation |

Review the package transaction, especially `dnf --allowerasing`, which permits
removing conflicting packages. The Debian package **recommends** `evdi-dkms`;
it is not a hard dependency and may not be installed when recommendations are
disabled. A working EVDI kernel module is still required for an extended display.

`makepkg -s` installs repository dependencies through pacman; it does not build
AUR packages. Install [evdi-dkms](https://aur.archlinux.org/packages/evdi-dkms)
separately first; see [makepkg(8)](https://man.archlinux.org/man/makepkg.8.en).
The recipe downloads its declared source tag, so it needs that tag to exist;
use the source workflow for an unreleased checkout.

Module and package availability depend on the distribution release and kernel.
Do not assume a Bazzite/Nobara image includes every prerequisite. The full
installer uses rpm-ostree on a detected booted ostree system and otherwise the
selected distribution's package manager. Layering may require a reboot.

| Distribution family | FFmpeg/adb names used by the project | EVDI source to check |
| --- | --- | --- |
| Debian/Ubuntu | `ffmpeg`, `adb` | `evdi-dkms` for the running kernel |
| Arch | `ffmpeg`, `android-tools` | AUR `evdi-dkms` |
| Fedora | `ffmpeg` (RPM Fusion in the workflow), `android-tools` | image/vendor packages or an upstream module build |
| openSUSE | `ffmpeg`, `android-tools` | `evdi` and its matching kernel-module package |

Use `modinfo evdi` and, for DKMS installations, `dkms status` to inspect the
installed module. If a DKMS build fails, inspect its build log and check the
module's compatibility with the running kernel and available headers. Follow
your distribution or [EVDI upstream](https://github.com/DisplayLink/evdi)
instructions for a compatible module; one pinned version is not a universal
kernel fix. A packaged userspace library is not the kernel module.

Portable tarballs/native packages bundle libevdi v1.15.0 next to the helper
and use an `$ORIGIN` lookup. A normal source installation may instead use the
system library installed during the build; see [development.md](development.md).

## Start the host

After a native package installation, on a desktop with a systemd user manager:

```bash
systemctl --user enable --now uscreen
```

The full tarball/source installer attempts to enable the service, but does
not start it. Start it with `systemctl --user start uscreen`. `make install`
only installs/reloads the user unit; enable/start it explicitly. Autostart
also depends on the desktop activating `graphical-session.target`; service-manager support remains limited
(T231), and custom XDG installation paths are inconsistent (T233).
Without a user service manager, `uscreen start` runs a foreground session;
arrange autostart through your desktop separately if needed.

If `~/.local/bin` is not on PATH yet, use `~/.local/bin/uscreen` or add the
directory to your shell's PATH. Run `uscreen doctor` to inspect the setup.
Select an encoder supported by your GPU/FFmpeg; the default is NVIDIA NVENC,
not automatic GPU detection.

For published artifacts, run `sha256sum -c SHA256SUMS` alongside all files
listed in the manifest. An absent file is reported as a verification failure.
See [release integrity](../SECURITY.md#release-integrity) for signing limitations.

## Android and first connection

1. Install the APK built from the same checkout (debug builds produce
   `android/app/build/outputs/apk/debug/app-debug.apk`). Android **8.1/API 27**
   is the minimum; decoding capabilities must also support the chosen stream.
2. Enable USB debugging in Developer options. On many tablets, tap *Build
   number* seven times under About to reveal that menu. Accept the computer's
   authorization prompt when connecting a data-capable USB cable.
3. With the host running, the daemon sets up adb forwarding and, by default,
   launches the app. Check desktop Display settings to enable/place the EVDI
   output if the desktop does not do so automatically. KDE Wayland has the
   most automation; see [compatibility](compatibility.md) for mapping limits.

UScreen defaults to 50% brightness and a 60 Hz display-mode preference only
while its window is in use. Change these in the app's gear menu; they do not
change other apps' system display settings. Stream FPS is a separate control.

System changes and removal instructions are in [SECURITY.md](../SECURITY.md).
For failures, use [troubleshooting.md](troubleshooting.md) before repeating setup.
