# Blent for Linux — Android Tablet as a USB Second Monitor

**Blent is an open-source SuperDisplay alternative for Linux.** It turns an
Android 8.1+ tablet into a real extended USB display and a pressure-sensitive
graphics tablet, with touch, S Pen pressure, tilt, eraser and stylus-button
support.

Blent is maintained at [geraldo-netto/UScreen](https://github.com/geraldo-netto/UScreen),
Original UScreen copyright (c) 2026 majmichu1; retained under the MIT license.
Historical benchmarks and compatibility reports describe upstream releases.
As of 2026-09-17 this fork has no published release. The source still reports
version 1.2.3; identify fork builds by their commit as well as that version.
The current development work is on `configurable-input-devices`.

Blent uses a direct ADB-over-USB connection — no Wi-Fi, USB tethering,
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

## Why Blent?

- **A real second monitor, not a mirror.** A virtual display is created
  through the EVDI kernel module; the tablet appears in your display settings
  and you move windows onto it.
- **Pen that works like a tablet.** Pressure, tilt, eraser and button arrive
  in Linux as a graphics-tablet device — Krita, GIMP and Blender see a tablet.
  Graphics-tablet mode uses the pen on a host screen without a streamed-video
  path. Input latency still depends on the device and desktop.
- **Historical upstream measurements.** About 22 ms median from encoded packet
  readiness to render acknowledgement over USB with
  H.264, 15–18 ms with HEVC, on the reference hardware — the
  [numbers and the method](docs/benchmarks.md) are published.
- **Connection automation.** With a configured user service and supported
  desktop, the daemon finds the tablet over adb, launches the app by default
  and negotiates geometry.
- **Local transport.** Loopback-only host ports, session authentication on by
  default, no application telemetry or account. See the trust boundaries and
  known limitations in [SECURITY.md](SECURITY.md).
- **Desktop-specific integration.** KDE Wayland has the most automation;
  X11 input mapping uses `xinput` and `xrandr`. Other Wayland desktops depend
  on compositor support. [Compatibility](docs/compatibility.md) lists known limits.

## Quick install

**1. Linux side** — for the current fork, follow the
[source-build instructions](docs/development.md#building-from-source). The
packaging tools can produce the files below; use them only when present on
the [fork releases page](https://github.com/geraldo-netto/UScreen/releases)
or produced from the checkout you intend to install:

| file | distribution |
| --- | --- |
| `blent-<ver>-x86_64.AppImage` | Linux x86-64/glibc 2.36+: `chmod +x blent-*.AppImage`; see [launch and user installation](docs/appimage-plan.md) |
| `blent-<ver>-1.x86_64.rpm` | openSUSE (`zypper install`), Fedora (RPM Fusion first, then `dnf install --allowerasing`) |
| `blent-<ver>-PKGBUILD.tar.gz` | Arch and derivatives — install AUR `evdi-dkms` first; extract, `makepkg -si` |
| `blent-<ver>-linux-x86_64.tar.gz` | compatible Linux x86-64/glibc systems — extract, inspect `./scripts/install.sh` |

Enable **Start Blent with the desktop** in the GUI for automatic login startup,
including on Cinnamon. The full installer enables autostart
through a user service or an XDG desktop entry; `make install` preserves that
preference. The GUI can toggle either route. Read
[installation details](docs/installation.md) first: full installer/native
package setup preserves loaded EVDI devices. Attaching a virtual display under
Cinnamon remains an unresolved crash risk (T222).

**2. Tablet** — install the APK from the same checkout (for a debug build,
`android/app/build/outputs/apk/debug/app-debug.apk`; release bundles use
`blent.apk`). Enable USB debugging in Developer options.

**3. Plug in.** The daemon sets up forwarding and launches the app by default. Check
desktop display settings and input mapping; `blent doctor` helps diagnose
setup problems. It cannot establish that every configuration is safe or supported.

Update both halves together: since 1.1.0 they share a session token.

If Blent replaced a second monitor for you, a star on the repo and a
[compatibility report](https://github.com/geraldo-netto/UScreen/issues/new?template=compatibility.yml)
help the next Linux user find it.

## Historical upstream compatibility

| host | tablet | result |
| --- | --- | --- |
| Bazzite, KDE Plasma 6 Wayland, NVIDIA RTX 5060 | Galaxy Tab S9 Ultra, Android 14 | works — reference setup, all benchmarks |
| Arch Linux, KDE Plasma Wayland | — | works — externally verified on a real system twice: the v1.0.2 installer (report (historical upstream)) and the v1.1.0 PKGBUILD via `makepkg -si`, with menu entry, tray and settings working out of the box (report (historical upstream)) |
| Fedora 44, KDE Plasma | Galaxy Tab S9 FE | works — "near perfectly", external report (discussion #7 (historical upstream)) |
| Debian 12 · Fedora 42 · openSUSE Tumbleweed | — | packages install and run (container-tested, no tablet) |

Android 8.1+ is the minimum; codec, profile, resolution and frame-rate support
also matter. These reports describe upstream builds, not current fork validation. More in
[docs/compatibility.md](docs/compatibility.md); reports are welcome.

## Performance

Historical upstream measurements on the reference hardware over USB
(2960×1848, 90 fps target, constant-quality encoding). Times run from encoded packet readiness to receipt
of the tablet's render acknowledgement; capture and encoding are excluded:

| | median | p95 |
| --- | --- | --- |
| H.264, NVENC | 18–22 ms | 23–31 ms |
| HEVC, NVENC | 15–18 ms | 20–23 ms |
| Wi-Fi fallback (H.264) | 22.8 ms | 78.6 ms, worst frames in seconds |

The tablet separately reported ~15 ms from frame arrival to render callback.
Subtracting independent medians does not measure transport or queueing time.
Method, CPU figures and measurement limits in
[docs/benchmarks.md](docs/benchmarks.md).

## Compared with the alternatives

The table compares documented setup choices, checked against the linked
primary sources on 2026-09-17. It is not a performance ranking or a complete
stylus-capability matrix; support depends on the host, client and versions.

| Project | Linux host | Display setup | Android connection |
| --- | --- | --- | --- |
| Blent | yes | EVDI virtual output; see [known limitations](docs/compatibility.md#current-fork-limitations) | native app, adb over USB; optional ADB over Wi-Fi |
| [SuperDisplay](https://superdisplay.app/help/) | no; Windows host | virtual extended display | native app, USB or Wi-Fi |
| [Weylus](https://github.com/H-M-H/Weylus#readme) | yes | capture a screen/window; configure a separate output for extension | browser over a network or `adb reverse` |
| [Sunshine](https://docs.lizardbyte.dev/projects/sunshine/latest/) + [Moonlight](https://github.com/moonlight-stream/moonlight-android) | yes | stream a host display; output provisioning depends on the host setup | native client over a network |
| [spacedesk](https://manual.spacedesk.net/AndroidUSBCableConnection.html) | no; Windows primary machine | virtual extended display | native app; direct Android USB is supported |

Blent supports NVENC and VAAPI hardware encoding and a **software** libx264
fallback. Consult each alternative's own documentation for its current
encoder, pen and licensing details.

## Settings

Host settings live in `~/.config/blent/config.toml`; edit them with
`blent-gui` or override supported settings with CLI flags. The tablet’s ⚙
sheet stores app preferences locally; **Apply** sends its shared streaming
settings to the host. Brightness/refresh preferences take effect immediately.
The tray controls the running daemon.
When `XDG_CONFIG_HOME` is an absolute path, host settings instead use
`$XDG_CONFIG_HOME/blent/config.toml`. Empty or relative values use the default.

- **Graphics tablet mode** — flip *Graphics tablet* on the tablet: nothing is
  streamed and the pen drives your own screen. Input latency remains. Switch back
  the same way. The connection overlay clears after the authenticated control
  greeting; it returns when the control connection is lost.
- **Tablet display** — brightness defaults to 50%, and the app requests 60 Hz.
  Adjust either in the gear menu; preferences persist and affect Blent only.
  Other apps retain normal system settings. Refresh selection uses the closest
  supported rate at the current display resolution; **System default** clears
  the request, and Android may override it. Stream FPS is separate.
- **Position** — `right` (default), `left`, `above`, `below` your real screens.
- **Orientation** — in the tablet's ⚙ sheet: rotate automatically with the
  tilt sensor, or pin *camera up* / *camera down*.
- **Codec** — new configurations default to `auto`: check tablet compatibility,
  measure available host encoders and, with current peers, compare bounded live
  render-ACK timing with a quality guard before accepting a selection. Saved
  explicit choices remain unchanged. Choose NVENC,
  VAAPI or a software encoder to override automatic selection. Linux settings
  can optionally
  [reuse a recent measured profile](docs/video-codecs.md#optional-historical-profile-cache)
  after fresh compatibility/render checks; this is off by default. VP9 and AV1
  (software or supported VAAPI/NVENC encoders) use tablet
  capability negotiation; see [codec compatibility](docs/video-codecs.md). HEVC is optional and needs a compatible tablet decoder.
  `h264_vaapi_baseline` is an explicit low-latency VAAPI H.264 profile; it trades
  higher bandwidth for lower decoder delay on the measured tablet. Existing
  selections stay unchanged; see [codec measurements](docs/benchmarks/2026-09-18-codecs.md).
  `ten_bit` requests HEVC Main10 on the FFmpeg path; capture is still 8-bit,
  and this does not enable HDR or guarantee less banding on every scene.
- **Bitrate and quality** — VAAPI uses uncapped constant quality (CQP). Its
  bitrate control is disabled on Linux; adjust **Quality** instead. The saved
  bitrate remains available for other encoders. **Auto** may select VAAPI and
  therefore may not enforce the configured bitrate. Android explains this
  limitation in its settings too; the value is not measured throughput.
- **Stream scale** — `stream_scale = 2` sends a quarter of the pixels for a
  historical ~6 ms lower packet-to-ack median on the reference tablet, at the
  cost of softer text; other devices differ.
- **Several tablets** — `max_tablets` supports 1–4 slots. Historical testing
  used one physical tablet plus a simulated client. Automated tests cover
  independent slots and busy-card fallback. Multiple physical tablets have not
  been validated; maintainer testing of that setup is outside the current scope (T382).
- **Input devices** — `input_touch`, `input_pen`, `input_pointer`: which
  virtual devices the desktop sees while a tablet is attached. All on by
  default; turn off what you do not use (on Cinnamon/GNOME under X11 a
  touchscreen device can hide the mouse cursor). The pointer requires Pen;
  disabling Pen preserves the pointer preference for when Pen is enabled again.
- **Wi-Fi** — `blent wifi` once, with the cable in: it switches the tablet
  over, remembers the address and reconnects to it by itself whenever the
  cable is out. `blent wifi --off` forgets the address and disconnects; it
  does not disable the tablet's network adb listener. See [SECURITY.md](SECURITY.md).
  USB preference also recognizes network ADB endpoints and mDNS wireless
  identifiers. See [benchmarks](docs/benchmarks.md)
  for historical Wi-Fi results.
- **Battery saver** — an opt-in Android setting that preserves brightness,
  refresh rate and stream settings. It releases unnecessary CPU/Wi-Fi locks on
  USB or while waiting, while retaining the network lock for active network
  sessions. It applies immediately; turning it off restores the normal policy.
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
proper tablet device. Automated coverage checks angular tilt units and
hover/button state across tip lift. Physical behavior depends on the tablet,
application and desktop mapping; see [compatibility](docs/compatibility.md).

**Does it work on Bazzite / KDE Wayland?** That is the historical upstream reference setup.
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
- [Tablet webcams on Linux](docs/cameras.md) — front/rear OS cameras, one active lens at a time.
- [Security](SECURITY.md) · [Changelog](CHANGELOG.md)

## Plans and proposals

The [Windows integration plan](docs/windows-port.md) separates compilation,
pen-only operation, extended-display support and packaging. It records pending
OS/driver/testing decisions; Windows host support is not implemented.

Other proposals inherited from upstream are AOA transport to reduce reliance
on USB debugging, broader Wayland input mapping, and a PipeWire/dmabuf capture
path. They have no committed fork schedule. Transport compatibility, compositor
support and performance improvements need implementation and validation.

HDR is not implemented; the current capture path is 8-bit. An iPad client is
also only a proposal and would need a supported transport, a native client,
build/signing resources and a distribution plan. No particular Apple
platform or regional distribution route has been selected or verified.

Current defects and blocked decisions remain in [TODO.md](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/TODO.md).

## Contributing

Compatibility reports are the most useful thing right now; see
[CONTRIBUTING.md](CONTRIBUTING.md). Questions and reports go to
[Issues](https://github.com/geraldo-netto/UScreen/issues).

## License

MIT — see [LICENSE](LICENSE). The bundled libevdi client library is LGPL-2.1
from DisplayLink, unmodified — see [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
