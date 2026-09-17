# UScreen for Linux — Android Tablet as a USB Second Monitor

**UScreen is an open-source SuperDisplay alternative for Linux.** It turns an
Android 8.1+ tablet into a real extended USB display and a pressure-sensitive
graphics tablet, with touch, S Pen pressure, tilt, eraser and stylus-button
support.

This fork is maintained at [geraldo-netto/UScreen](https://github.com/geraldo-netto/UScreen),
based on the [upstream project](https://github.com/majmichu1/UScreen) by majmichu1.
Historical benchmarks and linked compatibility reports describe upstream releases.

UScreen uses a direct ADB-over-USB connection — no Wi-Fi, USB tethering,
dummy HDMI plug or cloud account required. Screen and input data travel
between your computer and tablet over USB, or over your local network when
you enable the optional Wi-Fi fallback. They are not sent to a cloud service.

Tested on Bazzite (KDE Plasma, Wayland, NVIDIA) with a Samsung Galaxy Tab S9
Ultra. Packages and installation instructions cover Bazzite, Fedora,
Ubuntu/Debian, Arch Linux and openSUSE.

[**Download the latest release**](https://github.com/geraldo-netto/UScreen/releases/latest)
· [Install](#quick-install)
· [Compatibility](docs/compatibility.md)
· [Benchmarks](docs/benchmarks.md)
· [FAQ](#faq)
· [Website source](docs/index.html)

## Why UScreen?

- **A real second monitor, not a mirror.** A virtual display is created
  through the EVDI kernel module; the tablet appears in your display settings
  and you move windows onto it.
- **Pen that works like a tablet.** Pressure, tilt, eraser and button arrive
  in Linux as a graphics-tablet device — Krita, GIMP and Blender see a tablet.
  A one-tap *graphics tablet* mode uses the pen on your own screen with zero
  display latency.
- **Low latency, measured.** About 22 ms median from encoded packet
  readiness to render acknowledgement over USB with
  H.264, 15–18 ms with HEVC, on the reference hardware — the
  [numbers and the method](docs/benchmarks.md) are published.
- **Plug in and it works.** The daemon starts with your desktop, finds the
  tablet over adb, launches the app on it and sizes the display to its panel.
- **Private by construction.** Loopback-only ports guarded by a per-session
  token, no telemetry, no account. See [SECURITY.md](SECURITY.md).
- **Honest about its edges.** Wi-Fi is a fallback and the stutter is
  [quantified](docs/benchmarks.md#usb-vs-wi-fi-h264-quiet-link); KDE gets the
  full automation. X11 input maps automatically with `xinput` and `xrandr`;
  other Wayland desktops need manual input mapping.

## Quick install

**1. Linux side** — pick the file for your distribution from the
[latest release](https://github.com/geraldo-netto/UScreen/releases/latest):

| file | distribution |
| --- | --- |
| `uscreen_<ver>_amd64.deb` | Debian 12+, Ubuntu 24.04+, Mint 22+, Pop!_OS 24.04+ — `sudo apt install ./uscreen_*.deb` |
| `uscreen-<ver>-1.x86_64.rpm` | openSUSE (`zypper install`), Fedora (RPM Fusion first, then `dnf install --allowerasing`) |
| `uscreen-<ver>-PKGBUILD.tar.gz` | Arch and derivatives — install AUR `evdi-dkms` first; extract, `makepkg -si` |
| `uscreen-<ver>-linux-x86_64.tar.gz` | Bazzite, Nobara, anything else — extract, `./scripts/install.sh` |

Then `systemctl --user enable --now uscreen` (the tarball installer enables
it for you; start it once with `systemctl --user start uscreen`). Full
details, including what the installer changes on the system, in
[docs/installation.md](docs/installation.md).

**2. Tablet** — install `uscreen.apk` and enable USB debugging (Settings →
Developer options).

**3. Plug in.** The daemon forwards the ports, launches the app and the
tablet shows up as a monitor. `uscreen doctor` diagnoses anything that is off.

Update both halves together: since 1.1.0 they share a session token.

If UScreen replaced a second monitor for you, a star on the repo and a
[compatibility report](https://github.com/geraldo-netto/UScreen/issues/new?template=compatibility.yml)
help the next Linux user find it.

## Verified compatibility

| host | tablet | result |
| --- | --- | --- |
| Bazzite, KDE Plasma 6 Wayland, NVIDIA RTX 5060 | Galaxy Tab S9 Ultra, Android 14 | works — reference setup, all benchmarks |
| Arch Linux, KDE Plasma Wayland | — | works — externally verified on a real system twice: the v1.0.2 installer ([report](https://github.com/majmichu1/UScreen/issues/2#issuecomment-5478643599)) and the v1.1.0 PKGBUILD via `makepkg -si`, with menu entry, tray and settings working out of the box ([report](https://github.com/majmichu1/UScreen/issues/3#issuecomment-5494961262)) |
| Fedora 44, KDE Plasma | Galaxy Tab S9 FE | works — "near perfectly", external report ([discussion #7](https://github.com/majmichu1/UScreen/discussions/7)) |
| Debian 12 · Fedora 42 · openSUSE Tumbleweed | — | packages install and run (container-tested, no tablet) |

Any Android 8.1+ tablet with a hardware H.264 decoder should work — the
display is generated to match the tablet. More in
[docs/compatibility.md](docs/compatibility.md); reports are welcome.

## Performance

Measured on the reference hardware over USB (2960×1848, 90 fps target,
constant-quality encoding). Times run from encoded packet readiness to receipt
of the tablet's render acknowledgement; capture and encoding are excluded:

| | median | p95 |
| --- | --- | --- |
| H.264, NVENC | 18–22 ms | 23–31 ms |
| HEVC, NVENC | 15–18 ms | 20–23 ms |
| Wi-Fi fallback (H.264) | 22.8 ms | 78.6 ms, worst frames in seconds |

The tablet reports ~15 ms from frame arrival to render callback. The remaining
5–7 ms includes host queueing and both transport directions. Method, CPU figures
and measurement limits in
[docs/benchmarks.md](docs/benchmarks.md).

## Compared with the alternatives

The SuperDisplay host-platform entry follows its [official FAQ](https://superdisplay.app/help/)
(checked 2026-09-17). Other comparison entries below await revalidation.

| | UScreen | [SuperDisplay](https://superdisplay.app/) | [Weylus](https://github.com/H-M-H/Weylus) | [Sunshine](https://github.com/LizardByte/Sunshine) + Moonlight | [spacedesk](https://www.spacedesk.net/) |
| --- | --- | --- | --- | --- | --- |
| Linux host | **yes** | no (Windows) | yes | yes | no (Windows) |
| Real extended display | **yes** (EVDI) | yes | needs a separate virtual-display setup | needs an existing or dummy display | yes |
| Direct USB, no tethering | **yes** (adb) | yes | via adb port forward | no (network) | no (network) |
| S Pen pressure | **yes** | yes | yes | partial | partial |
| Tilt, eraser, button | **yes** | yes | pressure/tilt via browser API, no eraser | no | no |
| Hardware video encoding | NVENC / VAAPI / x264 | yes | yes (VAAPI/NVENC) | yes | yes |
| Open source | **MIT** | no | AGPL | GPL | no |
| Dummy HDMI plug | **no** | no | sometimes | often | no |

Also current: [MoreSpace](https://morespaceapp.com/) — a Linux host daemon
with an Android app, extended display by default, USB or Wi-Fi, pressure-
sensitive stylus; its documentation does not state tilt or eraser support, the
USB protocol, or a license. [TethrLink](https://github.com/princesavsaviya/TethrLink)
(GPL-3.0) — a real second monitor via GNOME's ScreenCast API, GNOME Wayland
only, over USB tethering, stylus not documented.
[subdisplay](https://github.com/TarikTopalovic/subdisplay) (MIT) — a wrapper
around Sunshine + Moonlight over USB tethering; a dummy plug for a true
extended display, pressure but no tilt.

## Settings

Host settings live in `~/.config/uscreen/config.toml`; edit them with
`uscreen-gui` or override supported settings with CLI flags. The tablet’s ⚙
sheet stores app preferences locally and sends shared streaming settings to
the host. The tray controls the running daemon.
When `XDG_CONFIG_HOME` is an absolute path, host settings instead use
`$XDG_CONFIG_HOME/uscreen/config.toml`. Empty or relative values use the default.

- **Graphics tablet mode** — flip *Graphics tablet* on the tablet: nothing is
  streamed, the pen drives your own screen, zero display latency. Switch back
  the same way; no restart.
- **Position** — `right` (default), `left`, `above`, `below` your real screens.
- **Orientation** — in the tablet's ⚙ sheet: rotate automatically with the
  tilt sensor, or pin *camera up* / *camera down*.
- **Codec** — `h264_nvenc` by default because every device decodes it;
  `hevc_nvenc` is sharper at the same bitrate and was faster on the reference
  tablet. `ten_bit` (HEVC Main10) smooths gradient banding — the desktop is
  8-bit, so it adds precision, not colour; it is not HDR.
- **Stream scale** — `stream_scale = 2` sends a quarter of the pixels for a
  ~6 ms lower decode time at the cost of softer text.
- **Several tablets** — `max_tablets` up to 4, each its own screen.
- **Input devices** — `input_touch`, `input_pen`, `input_pointer`: which
  virtual devices the desktop sees while a tablet is attached. All on by
  default; turn off what you do not use (on Cinnamon/GNOME under X11 a
  touchscreen device can hide the mouse cursor).
- **Wi-Fi** — `uscreen wifi` once, with the cable in: it switches the tablet
  over, remembers the address and reconnects to it by itself whenever the
  cable is out. `uscreen wifi --off` undoes it. The daemon prefers the cable
  when both are there, and the stutter is [quantified](docs/benchmarks.md).
- **Updates** — the app, the GUI and the tray tell you when a newer release
  exists; nothing installs itself. `check_updates = false` disables host checks;
  the tablet app has its own update-check switch.

## FAQ

**Does it need Wi-Fi or USB tethering?** No — a normal data-capable USB cable
with USB debugging enabled on the tablet; USB tethering is not needed. Wi-Fi
is an optional fallback.

**Is it a mirror or an extension?** An extension; a real monitor in your
display settings. Graphics-tablet mode is a separate, non-display mode.

**Does S Pen pressure and tilt work?** Yes, plus eraser and button, as a
proper tablet device.

**Does it work on Bazzite / KDE Wayland?** That is the reference setup.
X11 desktops get automatic input mapping with `xinput` and `xrandr`; place
outputs through desktop display settings. Other Wayland desktops, including
GNOME, require manual input mapping.

**Does it need a dummy HDMI plug?** No.

**Which Android versions?** 8.1 and newer.

**Is screen or input data sent to the cloud?** No. It travels over the USB
cable, or your local network when Wi-Fi fallback is enabled. The only automatic
internet request is an optional version check against GitHub
(`check_updates = false` turns it off; the app has a switch of its own).

**How do I uninstall it completely?** [SECURITY.md](SECURITY.md#how-to-uninstall-completely)
lists every file.

More in [docs/faq.md](docs/faq.md).

## Documentation

- [Installation](docs/installation.md) · [Troubleshooting](docs/troubleshooting.md)
- [Architecture and protocol](docs/architecture.md) · [Development, building, releasing](docs/development.md)
- [Benchmarks](docs/benchmarks.md) · [Compatibility](docs/compatibility.md) · [FAQ](docs/faq.md)
- [Security](SECURITY.md) · [Changelog](CHANGELOG.md)

## Roadmap

Shipped: extended display over USB, S Pen with pressure/tilt/eraser, graphics-
tablet mode switchable from the tablet, plug-and-play with autostart, tray
icon, any-side placement, several tablets, HEVC and 10-bit, Wi-Fi fallback,
packages for five distribution families, `uscreen doctor`, measured latency.

Next: **AOA transport** — removing the USB-debugging requirement, the last
step between this and simply plugging a cable in. Then automatic input mapping
on GNOME Wayland ([#4](https://github.com/majmichu1/UScreen/issues/4)), and a
**PipeWire/dmabuf capture path**: the EVDI cycle is serial by design (the
compositor copies the frame out of the GPU, then the helper copies it again,
then the compositor renders the next one), which caps native 2960×1848 at
about 60 frames/s on the reference laptop whatever the target; taking the
frame straight from the compositor as a GPU buffer would remove both copies.

Explored: HDR, currently blocked by EVDI providing only 8-bit framebuffers.
Considered, not scheduled: **iPad**. The Linux side would carry over (the
virtual display, the encoder, the input devices), but the transport would
have to move from adb to usbmuxd and the app would have to be rebuilt in
Swift. Since iOS 17.4 the EU's Digital Markets Act allows distribution
outside the App Store, which removes the review step, but not the rest: the
app still has to be notarised by Apple, which needs a paid developer account
and a Mac to build on, and outside the EU it stays App Store only. It is on
the list; it is not next.

## Contributing

Compatibility reports are the most useful thing right now; see
[CONTRIBUTING.md](CONTRIBUTING.md). Issues tagged `good first issue` are
self-contained. Questions go to
[Discussions](https://github.com/geraldo-netto/UScreen/discussions).

## License

MIT — see [LICENSE](LICENSE). The bundled libevdi client library is LGPL-2.1
from DisplayLink, unmodified — see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
