# Compatibility

The tables preserve **upstream** hardware reports, not a certification of
the current fork. “Maintainer” in those rows means the upstream maintainer.
Report your fork commit, installation method and hardware through the
[compatibility template](https://github.com/geraldo-netto/UScreen/issues/new?template=compatibility.yml).
Container installation/build checks are described in [development.md](development.md).

## Host

| distribution | desktop | GPU / encoder | result | source |
| --- | --- | --- | --- | --- |
| Bazzite (Fedora Atomic 42) | KDE Plasma 6, Wayland | NVIDIA RTX 5060 Laptop, `h264_nvenc` / `hevc_nvenc` | works; all measurements in [benchmarks.md](benchmarks.md) | maintainer |
| Arch Linux | KDE Plasma, Wayland | — | works — externally verified on a real system twice. v1.0.2: installed through `install.sh` plus the EVDI initialisation described in the issue; the application connected and ran successfully. v1.1.0: the PKGBUILD via `makepkg -si` with `evdi-dkms` from the AUR, no dependency or path issues; menu entry, tray icon and its Settings entry all work. A bug from that report — input staying on the laptop screen after leaving graphics-tablet mode ([issue #6](https://github.com/majmichu1/UScreen/issues/6)) — is fixed in 1.2.0 | [v1.0.2 report](https://github.com/majmichu1/UScreen/issues/2#issuecomment-5478643599), [v1.1.0 PKGBUILD report](https://github.com/majmichu1/UScreen/issues/3#issuecomment-5494961262) |
| KDE Neon (Plasma 6.7.5) | KDE Plasma, Wayland | AMD, `h264_vaapi` | works on 1.2.1 — touch lands on the tablet's own screen, confirmed by the reporter after the KWin D-Bus fix; evdi had to be installed by hand | [issue #9](https://github.com/majmichu1/UScreen/issues/9#issuecomment-5669072873), [#11](https://github.com/majmichu1/UScreen/issues/11) |
| Fedora 44 | KDE Plasma | — | works — "near perfectly" with a Galaxy Tab S9 FE, used for drawing; the reported orientation limitation is addressed by the current app’s automatic rotation and pinned landscape options | [discussion #7](https://github.com/majmichu1/UScreen/discussions/7) |
| Debian 12 | — | — | package installs and binaries run; not exercised with a tablet | maintainer, container |
| Fedora 42 | — | — | rpm installs; evdi must be built from source | maintainer, container |
| openSUSE Tumbleweed | — | — | dependencies resolve; not exercised with a tablet | maintainer, container |

## Desktop and encoder requirements

- **KDE Plasma on Wayland:** output placement uses `kscreen-doctor`, input
  mapping and keyboard suppression use KWin D-Bus. These tools/interfaces
  must be available; the historical matrix above is not a guarantee for all versions.
- **X11:** input mapping uses `xinput`/`xrandr`; manage output placement through
  desktop settings. Automatic KScreen placement is skipped.
  Cinnamon has an unresolved attachment-related Xorg crash (T222).
- **Other Wayland desktops:** support depends on that compositor's EVDI and
  input-mapping facilities. No blanket GNOME/Sway compatibility is established;
  manual configuration may be required or unavailable. Doctor reports manual
  mapping guidance instead of requiring KWin; display-only configurations skip
  input-mapping dependency checks.
- **Encoding:** new configurations use `auto`, which probes compatible stock
  FFmpeg encoders and checks render acknowledgements with H.264 fallback.
  Existing explicit choices remain unchanged. NVENC/VAAPI still require actual
  GPU encoding support; an advertised wrapper is insufficient. See
  [selection policy and measurement limits](video-codecs.md#automatic-selection).
- **Kernel:** a compatible EVDI module is needed for an extended output.
  Package/image availability varies; see [installation.md](installation.md).

## Tablet

| device | Android | stylus | result | source |
| --- | --- | --- | --- | --- |
| Samsung Galaxy Tab S9 Ultra | 14 | S Pen: pressure, tilt, eraser, button | works; HEVC Main10 decodes in hardware | maintainer |
| Samsung Galaxy Tab S9 FE | — | S Pen | works for drawing on a Fedora 44 KDE host | [discussion #7](https://github.com/majmichu1/UScreen/discussions/7) |
| Lenovo Tab K11 | 15 | Lenovo Tab Pen Plus | works on KDE Neon; 60–70 frames/s | [issue #11](https://github.com/majmichu1/UScreen/issues/11) |

Automatic rotation supports naturally portrait and naturally landscape panels;
landscape and reverse-landscape can also be pinned in Settings.

Android 8.1/API 27 is the minimum. The device must decode the requested codec,
profile, resolution and frame rate; version support alone does not guarantee
that. MediaCodec may choose software decoding. `uscreen doctor` reports codec
capabilities, but only a real stream validates the selected configuration.

## Current fork limitations

The full actionable list is [TODO.md](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/TODO.md). In particular:

- **T222:** Cinnamon/Xorg crashed during virtual-display attachment; cause and
  mitigation remain unverified. See the [incident report](reviews/2026-09-17-cinnamon-restart.md).
- The optional in-process build supports libx264/NVENC and explicitly rejects
  VAAPI configurations. Use the default FFmpeg subprocess build for VAAPI.
- **T332:** accepted dimensions/FPS can exceed the EDID pixel-clock limit;
  3840×2160 at 90 fps is one example. Lowering FPS can produce a valid mode.

Up to four slots can be configured. Historical multi-tablet validation used
one physical tablet plus a simulated client. Automated tests cover busy-card
fallback, independent slot ownership and one/two/four simulated tablets; these
do not establish compatibility with four physical displays. Stylus tilt units
and hover/button transitions have permanent regression coverage, while
physical stylus behavior still depends on the device and desktop mapping.

## Not supported

- Windows or macOS hosts. [SuperDisplay](https://superdisplay.app/help/) is a
  Windows-host alternative, not a macOS-host alternative.
- iPads.
- Android below 8.1.
