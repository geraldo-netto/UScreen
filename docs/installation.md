# Installing UScreen for Linux

Build the Linux daemon/GUI/helper and Android app from the same checkout.
As of 2026-09-17 this fork has no published releases; start with
[building from source](development.md#building-from-source). Upstream releases
do not contain the unreleased fork changes. The instructions below also cover
artifacts produced locally or available in a future [fork release](https://github.com/geraldo-netto/UScreen/releases).

Before attaching a display, check [current limitations](compatibility.md#current-fork-limitations).
A Cinnamon/Xorg session crash during EVDI attachment remains unresolved (T222).
The full installer and native package hooks preserve loaded EVDI devices.
They load the module if needed and add missing capacity up to two devices;
failed provisioning reports that setup must be checked and deferred to reboot.
Boot configuration changes take effect on the next module load.
`make setup-system` uses the same provisioning script. GUI setup also adds
only missing devices, using its configured tablet count.

## Linux artifacts and prerequisites

The portable packaging workflow targets **Linux x86-64, glibc 2.36 or newer**.
This is an ABI baseline, not a guarantee that every distribution, GPU or
compositor works. Local builds can require a newer glibc. Runtime needs adb,
FFmpeg with the chosen encoder, compatible EVDI kernel support, libdrm, GUI
libraries and access to `/dev/uinput`. Container tests cannot validate kernel
attachment or tablet operation.

| Artifact | Installation |
| --- | --- |
| `uscreen-<ver>-x86_64.AppImage` | Linux x86-64/glibc 2.36+: make executable and launch; see [AppImage setup, extraction and migration](appimage-plan.md) |
| `uscreen-<ver>-1.x86_64.rpm` | openSUSE: `sudo zypper install ./uscreen-*.rpm`; Fedora: supply the required FFmpeg package (the build workflow uses RPM Fusion), then `sudo dnf install --allowerasing ./uscreen-*.rpm` |
| `uscreen-<ver>-PKGBUILD.tar.gz` | Arch family: install AUR `evdi-dkms` first, extract the recipe and run `makepkg -si` |
| `uscreen-<ver>-linux-x86_64.tar.gz` | Other compatible Linux setups: extract and inspect/run `./scripts/install.sh`; unsupported package managers need manual dependency installation |

Review the package transaction, especially `dnf --allowerasing`, which permits
removing conflicting packages. AppImage replaces the Debian release asset and
bundles userspace dependencies; it does not install EVDI/DKMS, GPU drivers or
device permissions. A working EVDI kernel module is still required for an
extended display. Install the appropriate module package for your kernel.

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
| RHEL/CentOS/Rocky/AlmaLinux | `ffmpeg`, `android-tools` from repositories configured for the installed Enterprise Linux release | distribution/vendor guidance or an upstream module build |
| openSUSE | `ffmpeg`, `android-tools` | `evdi` and its matching kernel-module package |

The installer matches complete `ID`/`ID_LIKE` words and gives Enterprise Linux
precedence when its metadata also mentions Fedora. It tries that system's
configured repositories and prints the detected release; if packages are
missing, configure repositories for that Enterprise Linux major release before
retrying. RPM Fusion publishes separate [Enterprise Linux](https://download1.rpmfusion.org/free/el/)
and [Fedora](https://download1.rpmfusion.org/free/fedora/) repository packages.
The installer only attempts Fedora repository setup for a Fedora-family system
with a numeric Fedora release macro. Booted ostree systems use package layering
from configured repositories. Unknown families require manual installation of
the runtime prerequisites listed above, using their own package manager.

Use `modinfo evdi` and, for DKMS installations, `dkms status` to inspect the
installed module. If a DKMS build fails, inspect its build log and check the
module's compatibility with the running kernel and available headers. Follow
your distribution or [EVDI upstream](https://github.com/DisplayLink/evdi)
instructions for a compatible module; one pinned version is not a universal
kernel fix. A packaged userspace library is not the kernel module.

Portable tarballs/native packages bundle libevdi v1.15.0 next to the helper
and use an `$ORIGIN` lookup. A normal source installation may instead use the
system library installed during the build; see [development.md](development.md).
The Debian-family installer installs FFmpeg/ADB and GUI libraries independently
from libevdi and DKMS. It skips a libevdi package when the required library is
already present: a bundle/system `libevdi.so.1` for prebuilt programs, or the
system `libevdi.so` linker file for a source build. Standard system library
locations and `/usr/local/lib`, `/usr/local/lib64` and their architecture
subdirectories are checked. Otherwise it requests `libevdi1` for prebuilt
programs or `libevdi-dev` for source builds. The kernel module remains a separate
requirement even when the bundle supplies its userspace library.

## Start the host

After a native package installation, on a desktop with a systemd user manager:

```bash
systemctl --user enable --now uscreen
```

The full tarball/source installer enables the user service when it can reach
the user manager, and otherwise creates an XDG desktop autostart entry. It
reports the selected route and does not start the daemon during installation.
`make install` installs user files while preserving the autostart preference.
The GUI's **Start UScreen with the desktop** setting enables/disables the
available route and starts/stops the current daemon.

Systemd autostart depends on the desktop activating `graphical-session.target`.
The fallback uses `XDG_CONFIG_HOME/autostart/uscreen.desktop` (default
`~/.config/autostart/uscreen.desktop`) on desktops implementing the
[XDG autostart specification](https://specifications.freedesktop.org/autostart/latest/).
It runs a direct daemon without service restart supervision. `uscreen start`
also runs a foreground session from a terminal. Enabling the systemd route
removes UScreen's fallback entry to avoid duplicate startup.
If systemctl still reports an enabled unit while its manager is unreachable,
autostart changes report an error; restore the user manager before switching
routes or disabling that unit.

Autostart does not provide kernel modules or input permissions. On another
init system, configure `evdi` and `uinput` to load at boot; do not assume it
reads `/etc/modules-load.d`. For example, OpenRC supplies a `modules` setting
in [`/etc/conf.d/modules`](https://github.com/OpenRC/openrc/blob/master/conf.d/modules);
add these modules to the existing list using your distribution's service setup.
The installer writes modprobe options and a udev rule. A device manager without
udev-compatible rules needs its own `/dev/uinput` access configuration; verify
module availability and permissions with `uscreen doctor` before streaming.

Both user installers share the same paths: launchers and icons go under
`XDG_DATA_HOME` (default `~/.local/share`), and the user unit goes under
`XDG_CONFIG_HOME/systemd/user` (default `~/.config/systemd/user`). Empty or
relative XDG values use those defaults, matching UScreen's configuration-path
policy. Program binaries remain in `~/.local/bin`; these XDG overrides do not
change that location. Native packages use their system-wide package paths.
For a custom program directory, use `make install BIN_DIR=/absolute/path`.
The generated launcher and user service both refer to that selected directory,
including the service's helper and stop commands.

If `~/.local/bin` is not on PATH yet, use `~/.local/bin/uscreen` or add the
directory to your shell's PATH. Run `uscreen doctor` to inspect the setup.
New configurations use [automatic encoder selection](video-codecs.md#automatic-selection)
after tablet negotiation, with `libx264` fallback. Existing explicit preferences
remain unchanged; choose a supported encoder or `auto` in Linux settings.

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

## Capture pipe capacity

Linux **Video → Capture pipe buffer** offers 1, 2, 4 and 8 MiB, with 1 MiB as
the default. Pipe-only edits apply without restarting the display. The GUI shows
the actual capacity and includes copyable instructions for raising the kernel's
limit. See [capture pipe configuration](pipe-buffer.md) for temporary/persistent
commands, rollback and the latency tradeoff.
