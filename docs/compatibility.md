# Compatibility

What UScreen has actually been run on. Rows come from the maintainer and from
[compatibility reports](https://github.com/geraldo-netto/UScreen/issues?q=label%3Acompatibility);
please add yours.

## Host

| distribution | desktop | GPU / encoder | result | source |
| --- | --- | --- | --- | --- |
| Bazzite (Fedora Atomic 42) | KDE Plasma 6, Wayland | NVIDIA RTX 5060 Laptop, `h264_nvenc` / `hevc_nvenc` | works; all measurements in [benchmarks.md](benchmarks.md) | maintainer |
| Arch Linux | KDE Plasma, Wayland | — | works — externally verified on a real system twice. v1.0.2: installed through `install.sh` plus the EVDI initialisation described in the issue; the application connected and ran successfully. v1.1.0: the PKGBUILD via `makepkg -si` with `evdi-dkms` from the AUR, no dependency or path issues; menu entry, tray icon and its Settings entry all work. A bug from that report — input staying on the laptop screen after leaving graphics-tablet mode ([issue #6](https://github.com/majmichu1/UScreen/issues/6)) — is fixed in 1.2.0 | [v1.0.2 report](https://github.com/majmichu1/UScreen/issues/2#issuecomment-5478643599), [v1.1.0 PKGBUILD report](https://github.com/majmichu1/UScreen/issues/3#issuecomment-5494961262) |
| KDE Neon (Plasma 6.7.5) | KDE Plasma, Wayland | AMD, `h264_vaapi` | works on 1.2.1 — touch lands on the tablet's own screen, confirmed by the reporter after the KWin D-Bus fix; evdi had to be installed by hand | [issue #9](https://github.com/majmichu1/UScreen/issues/9#issuecomment-5661447326), [#11](https://github.com/majmichu1/UScreen/issues/11) |
| Fedora 44 | KDE Plasma | — | works — "near perfectly" with a Galaxy Tab S9 FE, used for drawing; the reported orientation limitation is addressed by the current app’s automatic rotation and pinned landscape options | [discussion #7](https://github.com/majmichu1/UScreen/discussions/7) |
| Debian 12 | — | — | package installs and binaries run; not exercised with a tablet | maintainer, container |
| Fedora 42 | — | — | rpm installs; evdi must be built from source | maintainer, container |
| openSUSE Tumbleweed | — | — | dependencies resolve; not exercised with a tablet | maintainer, container |

Requirements that follow from the design:

- **Wayland with KDE Plasma** gets the full experience: the daemon places the
  virtual output and maps the pen and touch onto it through KWin's D-Bus
  interfaces, and suppresses the on-screen keyboard.
- **X11** (Cinnamon, XFCE, MATE, GNOME on Xorg): the daemon maps each
  tablet's input to its output with `xinput` and `xrandr`; output placement
  remains in the desktop's display settings.
- **Other Wayland desktops** (GNOME, Sway): the display and stream work wherever
  EVDI does; assign input and output placement in the desktop's settings.
  See [troubleshooting](troubleshooting.md#touch-or-pen-land-on-the-wrong-screen).
  Reports welcome.
- **NVIDIA** uses NVENC; **AMD/Intel** use VAAPI (`h264_vaapi`); anything can
  fall back to `libx264` on the CPU.
- The evdi kernel module must be available: in the image (Bazzite, Nobara),
  from the repositories (Debian, Ubuntu, openSUSE), from the AUR (Arch) or
  built from source (Fedora).

## Tablet

| device | Android | stylus | result | source |
| --- | --- | --- | --- | --- |
| Samsung Galaxy Tab S9 Ultra | 14 | S Pen: pressure, tilt, eraser, button | works; HEVC Main10 decodes in hardware | maintainer |
| Samsung Galaxy Tab S9 FE | — | S Pen | works for drawing on a Fedora 44 KDE host | [discussion #7](https://github.com/majmichu1/UScreen/discussions/7) |
| Lenovo Tab K11 | 15 | Lenovo Tab Pen Plus | works on KDE Neon; 60–70 frames/s | [issue #11](https://github.com/majmichu1/UScreen/issues/11) |

Any Android 8.1+ device with a hardware H.264 decoder should work — the app
reports its own panel size and the virtual display is generated to match.
HEVC is optional and only worth enabling where `uscreen doctor` reports a
hardware HEVC decoder.

## Not supported

- Windows or macOS hosts. [SuperDisplay](https://superdisplay.app/help/) is a
  Windows-host alternative, not a macOS-host alternative.
- iPads.
- Android below 8.1.
