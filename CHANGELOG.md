# Changelog

Full notes for each version are on the
[releases page](https://github.com/majmichu1/UScreen/releases).

## Unreleased

- The settings window now identifies itself to the desktop as `uscreen`, the
  same name as its menu entry, so the KDE task bar shows the UScreen icon for
  it instead of a generic monitor.

## 1.2.0 — 2026-09-12

- Performance: ffmpeg was converting every frame through RGB on the CPU
  because the BT.709 tags were given as output options, which ffmpeg 7+
  reads as a request to convert. Tagging the input instead drops the
  encoder process from about four cores to under half a core at 60 fps and
  removes a needless colour round-trip.
- Capture: the helper asks the compositor for the next frame as soon as the
  previous one is copied out instead of waiting for the next period tick,
  and keeps the framebuffer on huge pages, which cuts the copy from about
  6.5 ms to 4.5 ms at 2960×1848. Under motion that is 58–63 frames/s where
  it was 52–57 (at a 90 fps target), and a 30 fps target no longer lands
  at 24. The 5-second stats line now also shows how long the compositor
  took to answer and how long the copy took, which is how this was found.
- Icons: the app, the menu entry, the settings window and the tray now share
  one UScreen icon (amber tablet and stylus on charcoal) instead of the stock
  Android tile and a generic display glyph; the tray shows a dimmed variant
  in graphics-tablet mode.
- `uscreen doctor` warns when Samsung's Motion smoothness is on Standard,
  which holds the panel at 60 Hz whatever the app asks for.
- App: *Rotate automatically* in the ⚙ sheet follows the tilt sensor between
  the two landscape directions, so the tablet can be held camera-down for
  drawing (requested in
  [discussion #7](https://github.com/majmichu1/UScreen/discussions/7)); with
  it off, *Camera up* / *Camera down* pin the direction. The app reads the
  sensor itself because the system's own sensor mode never flipped the
  reference tablet.
- Fix: after leaving graphics-tablet mode the pen and touch could stay mapped
  to the laptop screen. The daemon now waits for the virtual display to be
  enabled before mapping the input devices and verifies the mapping instead of
  assuming it ([#6](https://github.com/majmichu1/UScreen/issues/6)).

## 1.1.0 — 2026-08-31

- Security: session token between app and daemon; capture FIFO moved out of
  `/tmp` into the per-user runtime directory.
- Packages: `.deb`, `.rpm`, PKGBUILD archive and `SHA256SUMS` in every release;
  binaries built against Debian 12 glibc so they run on any current
  distribution.
- Several tablets at once (`max_tablets`), each as its own screen.
- Update checks in the app, the GUI and the tray (report only).
- udev rule for `/dev/uinput`, so input works outside Bazzite.
- Fixes: PID-file race between daemon restarts, `uscreen stop` matching
  unrelated processes, GUI tablet detection with two adb devices, PATH-free
  menu and tray launching, atomic config writes with change logging.
- Minimum Android is 8.1 (it always was, in practice).

## 1.0.2 — 2026-08-30

- openSUSE support; userspace and kernel-module installs split so one failing
  package does not take the rest down; PATH check.

## 1.0.1 — 2026-08-30

- Fixes for issue #2: correct package names on Arch (AUR), Debian and Fedora;
  the daemon explains a missing EVDI device instead of retrying forever.

## 1.0.0 — 2026-08-26

- Wi-Fi as a fallback transport with a low-latency Wi-Fi lock, system tray
  icon, virtual screen on any side of the desktop, HEVC and 10-bit encoding.

## 0.4.0 — 2026-08-26

- Graphics-tablet mode switchable from the tablet; capture helper no longer
  spins a core when the output is disabled.

## 0.3.0 — 2026-08-25

- `uscreen doctor`, end-to-end latency measurement, input mapped to the
  virtual display (issue #1), optional in-process encoder.
