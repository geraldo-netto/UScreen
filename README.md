# UScreen for Linux — Android Tablet as a USB Second Monitor

**UScreen is an open-source SuperDisplay alternative for Linux.** It turns an
Android 8.1+ tablet into a real extended USB display and a pressure-sensitive
graphics tablet, with touch, S Pen pressure, tilt, eraser and stylus-button
support.

This fork is maintained at [geraldo-netto/UScreen](https://github.com/geraldo-netto/UScreen),
based on the [upstream project](https://github.com/majmichu1/UScreen) by majmichu1.
Historical benchmarks and linked compatibility reports describe upstream releases.
As of 2026-09-17 this fork has no published release. The source still reports
version 1.2.3; identify fork builds by their commit as well as that version.
The current development work is on `configurable-input-devices`.

UScreen uses a direct ADB-over-USB connection — no Wi-Fi, USB tethering,
dummy HDMI plug or cloud account required. Screen and input data travel
between your computer and tablet over USB, or over your local network when
you enable the optional Wi-Fi fallback. They are not sent to a cloud service.

Upstream tests used Bazzite (KDE Plasma, Wayland, NVIDIA) with a Samsung
Galaxy Tab S9 Ultra. The fork has known limitations, including an unresolved
Cinnamon/Xorg restart during attachment; read [current limitations](docs/compatibility.md#current-fork-limitations)
before setup. Packaging recipes target several Linux distribution families.

[**Build from source**](docs/development.md#building-from-source)
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
  A one-tap *graphics tablet* mode uses the pen on your own screen with no streamed-video decode/display path. Input latency remains.
- **Low latency, measured.** About 22 ms median from encoded packet
  readiness to render acknowledgement over USB with
  H.264, 15–18 ms with HEVC, on the reference hardware — the
  [numbers and the method](docs/benchmarks.md) are published.
- **Plug in and it works.** With a configured user service and supported desktop, the daemon finds the
  tablet over adb, launches the app on it and sizes the display to its panel.
- **Local transport.** Loopback-only host ports, session authentication on by
  default, no application telemetry or account. See the trust boundaries and
  known limitations in [SECURITY.md](SECURITY.md).
- **Honest about its edges.** Wi-Fi is a fallback and the stutter is
  [quantified](docs/benchmarks.md#usb-vs-wi-fi-h264-quiet-link); KDE gets the
  full automation. X11 input maps automatically with `xinput` and `xrandr`;
  other Wayland desktops need manual input mapping.

## Quick install

**1. Linux side** — for the current fork, follow the
[source-build instructions](docs/development.md#building-from-source). The
packaging tools can produce the files below; use them only when present on
the [fork releases page](https://github.com/geraldo-netto/UScreen/releases)
or produced from the checkout you intend to install:

| file | distribution |
| --- | --- |
| `uscreen_<ver>_amd64.deb` | Debian 12+, Ubuntu 24.04+, Mint 22+, Pop!_OS 24.04+ — `sudo apt install ./uscreen_*.deb` |
| `uscreen-<ver>-1.x86_64.rpm` | openSUSE (`zypper install`), Fedora (RPM Fusion first, then `dnf install --allowerasing`) |
| `uscreen-<ver>-PKGBUILD.tar.gz` | Arch and derivatives — install AUR `evdi-dkms` first; extract, `makepkg -si` |
| `uscreen-<ver>-linux-x86_64.tar.gz` | compatible Linux x86-64/glibc systems — extract, inspect `./scripts/install.sh` |

Then `systemctl --user enable --now uscreen` (the tarball installer enables
it for you; start it once with `systemctl --user start uscreen`). Full
details, including what the installer changes on the system, in
[docs/installation.md](docs/installation.md).

**2. Tablet** — install `uscreen.apk` and enable USB debugging (Settings →
Developer options).

**3. Plug in.** The daemon sets up forwarding and launches the app by default. Check
desktop display settings and input mapping; `uscreen doctor` helps diagnose
setup problems. It cannot establish that every configuration is safe or supported.

Update both halves together: since 1.1.0 they share a session token.

If UScreen replaced a second monitor for you, a star on the repo and a
[compatibility report](https://github.com/geraldo-netto/UScreen/issues/new?template=compatibility.yml)
help the next Linux user find it.

## Historical upstream compatibility

| host | tablet | result |
| --- | --- | --- |
| Bazzite, KDE Plasma 6 Wayland, NVIDIA RTX 5060 | Galaxy Tab S9 Ultra, Android 14 | works — reference setup, all benchmarks |
| Arch Linux, KDE Plasma Wayland | — | works — externally verified on a real system twice: the v1.0.2 installer ([report](https://github.com/majmichu1/UScreen/issues/2#issuecomment-5478643599)) and the v1.1.0 PKGBUILD via `makepkg -si`, with menu entry, tray and settings working out of the box ([report](https://github.com/majmichu1/UScreen/issues/3#issuecomment-5494961262)) |
| Fedora 44, KDE Plasma | Galaxy Tab S9 FE | works — "near perfectly", external report ([discussion #7](https://github.com/majmichu1/UScreen/discussions/7)) |
| Debian 12 · Fedora 42 · openSUSE Tumbleweed | — | packages install and run (container-tested, no tablet) |

Android 8.1+ is the minimum; codec, profile, resolution and frame-rate support
also matter. These reports describe upstream builds, not current fork validation. More in
[docs/compatibility.md](docs/compatibility.md); reports are welcome.

## Performance

Historical upstream measurements on the reference hardware over USB
(2960×1848, 90 fps target,
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

The table compares documented setup choices, checked against the linked
primary sources on 2026-09-17. It is not a performance ranking or a complete
stylus-capability matrix; support depends on the host, client and versions.

| Project | Linux host | Display setup | Android connection |
| --- | --- | --- | --- |
| UScreen | yes | EVDI virtual output; see [known limitations](docs/compatibility.md#current-fork-limitations) | native app, adb over USB; optional ADB over Wi-Fi |
| [SuperDisplay](https://superdisplay.app/help/) | no; Windows host | virtual extended display | native app, USB or Wi-Fi |
| [Weylus](https://github.com/H-M-H/Weylus#readme) | yes | capture a screen/window; configure a separate output for extension | browser over a network or `adb reverse` |
| [Sunshine](https://docs.lizardbyte.dev/projects/sunshine/latest/) + [Moonlight](https://github.com/moonlight-stream/moonlight-android) | yes | stream a host display; output provisioning depends on the host setup | native client over a network |
| [spacedesk](https://manual.spacedesk.net/AndroidUSBCableConnection.html) | no; Windows primary machine | virtual extended display | native app; direct Android USB is supported |

UScreen supports NVENC and VAAPI hardware encoding and a **software** libx264
fallback. Consult each alternative's own documentation for its current
encoder, pen and licensing details.

## Settings

Host settings live in `~/.config/uscreen/config.toml`; edit them with
`uscreen-gui` or override supported settings with CLI flags. The tablet’s ⚙
sheet stores app preferences locally; **Apply** sends its shared streaming
settings to the host. Brightness/refresh preferences take effect immediately.
The tray controls the running daemon.
When `XDG_CONFIG_HOME` is an absolute path, host settings instead use
`$XDG_CONFIG_HOME/uscreen/config.toml`. Empty or relative values use the default.

- **Graphics tablet mode** — flip *Graphics tablet* on the tablet: nothing is
  streamed and the pen drives your own screen. Input latency remains. Switch back
  the same way. A current greeting mismatch can leave its reconnect overlay
  visible (T247); see [known limitations](docs/compatibility.md#current-fork-limitations).
- **Tablet display** — brightness defaults to 50%, and the app requests 60 Hz.
  Adjust either in the gear menu; preferences persist and affect UScreen only.
  Other apps retain normal system settings. Refresh selection uses the closest
  supported rate at the current display resolution; **System default** clears
  the request, and Android may override it. Stream FPS is separate.
- **Position** — `right` (default), `left`, `above`, `below` your real screens.
- **Orientation** — in the tablet's ⚙ sheet: rotate automatically with the
  tilt sensor, or pin *camera up* / *camera down*.
- **Codec** — `h264_nvenc` is the configured default and requires working
  NVIDIA NVENC. Select VAAPI for a supported AMD/Intel setup or `libx264` for
  software encoding. HEVC is optional and needs a compatible tablet decoder.
  `ten_bit` requests HEVC Main10 on the FFmpeg path; capture is still 8-bit,
  and this does not enable HDR or guarantee less banding on every scene.
- **Stream scale** — `stream_scale = 2` sends a quarter of the pixels for a
  historical ~6 ms lower packet-to-ack median on the reference tablet, at the
  cost of softer text; other devices differ.
- **Several tablets** — `max_tablets` supports 1–4 slots. Historical testing
  used one physical tablet plus a simulated client; card-allocation limitations
  remain (T330).
- **Input devices** — `input_touch`, `input_pen`, `input_pointer`: which
  virtual devices the desktop sees while a tablet is attached. All on by
  default; turn off what you do not use (on Cinnamon/GNOME under X11 a
  touchscreen device can hide the mouse cursor).
- **Wi-Fi** — `uscreen wifi` once, with the cable in: it switches the tablet
  over, remembers the address and reconnects to it by itself whenever the
  cable is out. `uscreen wifi --off` forgets the address and disconnects; it
  does not disable the tablet's network adb listener. See [SECURITY.md](SECURITY.md).
  The daemon prefers the cable
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
outputs through desktop display settings. Other Wayland desktops depend on
compositor support and manual mapping; see [compatibility](docs/compatibility.md).

**Does it need a dummy HDMI plug?** No.

**Which Android versions?** 8.1 and newer.

**Is screen or input data sent to the cloud?** No. It travels over the USB
cable, or your local network when Wi-Fi fallback is enabled. The only automatic
internet request is an optional version check against GitHub
(`check_updates = false` turns it off; the app has a switch of its own).

**How do I uninstall it completely?** [SECURITY.md](SECURITY.md#how-to-uninstall-completely)
describes application files, custom paths and remaining system state.

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
self-contained. Questions and reports go to
[Issues](https://github.com/geraldo-netto/UScreen/issues).

## License

MIT — see [LICENSE](LICENSE). The bundled libevdi client library is LGPL-2.1
from DisplayLink, unmodified — see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
