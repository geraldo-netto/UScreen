# Windows host support — sizing notes

Status: design notes only, nothing implemented. Written 2026-09-16 against
1.2.3 to size the work before starting it.

## What stays the same

- **The tablet app.** It receives an H.264/HEVC stream over a loopback TCP
  port and sends touch/pen events back over a WebSocket. It has no idea what
  the host runs. No change.
- **The adb transport.** adb runs on Windows; `adb reverse` works the same.
  The discovery loop in `host/src/main.rs` is portable apart from the
  process-management helpers.
- **The ffmpeg encoder process.** Same binary, different encoder names:
  `h264_nvenc` (NVIDIA), `h264_amf` (AMD), `h264_qsv` (Intel), `libx264`
  as the CPU fallback. The argument builder in `capture.rs` gains three
  more branches.
- **Config, stream server, latency tracking, update check.** Portable.

## What has to be replaced

| Linux piece | Where | Windows replacement |
|---|---|---|
| EVDI virtual display + `evdi_helper` frame grab | `capture.rs`, `vdisplay.rs`, `edid.rs` | An Indirect Display Driver (IDD). Build on [VirtualDrivers/Virtual-Display-Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver) (MIT) rather than writing one: it already exposes an adapter with configurable modes. Capture the new monitor with the Desktop Duplication API (DXGI), or directly from the IDD swap-chain if we ship our own driver. Frames arrive as D3D11 textures; NVENC/AMF/QSV can take them without a CPU copy. |
| `mkfifo` + raw frame pipe to ffmpeg | `capture.rs` | Named pipe (`\\.\pipe\...`) or, better, the in-process `inproc-encoder` feature so no pipe is needed. |
| uinput touch / pen / pointer devices | `input.rs` | `SendInput` for the parking pointer and mouse clicks; the Pointer Injection API (`CreateSyntheticPointerDevice` + `InjectSyntheticPointerInput`, Windows 10 1809+) for real multitouch and for pen with pressure, tilt and eraser. No driver, no admin. |
| KWin D-Bus output mapping, on-screen keyboard suppression | `kwin.rs`, `osk.rs`, `input.rs` | Not needed: injected pointer input is already given in screen coordinates of a chosen monitor (`GetMonitorInfo` + the virtual monitor's rect). |
| systemd user unit, `pgrep`/`pkill`, PID file | `main.rs`, `runtime.rs` | Startup-folder entry or Task Scheduler task; a named mutex for single-instance. |
| `ksni` tray icon | `tray.rs` | `tray-icon` crate (cross-platform) or `windows` crate `Shell_NotifyIcon`. |
| `uscreen doctor` checks (`/dev/uinput`, evdi module, busctl) | `doctor.rs` | Driver installed? Encoder present? Injection API available? |

## Suggested order

1. **Factor a platform trait first, on Linux only.** Something like
   `VirtualDisplay` (create/destroy, mode, position), `FrameSource`
   (next frame), `InputSink` (touch/pen/pointer), `Session` (autostart,
   single instance). Move the Linux code behind it without changing
   behaviour. This is the pull request worth sending upstream.
2. Windows `InputSink` via pointer injection: smallest piece, testable with
   the existing app against any monitor (pen-only mode works without a
   virtual display at all).
3. Windows `VirtualDisplay` + `FrameSource` using the IDD driver and DXGI
   duplication.
4. Encoder branches for AMF/QSV, Windows tray and autostart, doctor checks,
   an MSI or winget package.

## Open questions

- Ship our own signed IDD (needs an EV code-signing certificate for
  attestation signing) or depend on the user installing Virtual-Display-Driver
  separately? Depending on it is the realistic first version.
- `ffmpeg-next` on Windows: prebuilt ffmpeg via vcpkg or a bundled DLL set.
- Wi-Fi mode has no adb dependency and would work unchanged.
