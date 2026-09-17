# Windows integration plan

Status: proposed plan, saved 2026-09-17. Windows support is not implemented.
The recommendations below remain pending decisions.

Delivery sequence: **Windows compilation → pen-only operation → extended
display → packaged release**. Each milestone has separate acceptance checks;
a successful Windows build alone does not establish functional support.

## Decisions and resources needed

| Decision or resource | Recommendation | Condition to proceed |
|---|---|---|
| Windows target | Windows 11 x64 first | Confirm the initial OS and architecture. Decide separately whether Windows 10 or ARM64 belongs in the first release. |
| Virtual-display driver | Integrate an existing [Virtual Display Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver) installation | Confirm that a separately installed driver is acceptable. Pin and validate its version and control interface before implementing display integration. |
| Hardware testing | A Windows PC connected to the Android tablet | Provide a test machine or tester, its Windows version and GPU, and administrator access for driver installation when authorized. CI or a VM can cover builds and isolated tests; USB, GPU and tablet validation need physical hardware. |
| Owned driver, if required | Treat driver ownership as a separate deliverable | Confirm maintenance, signing and distribution responsibilities before developing or shipping our own driver. |
| Application distribution | Per-user application with optional autostart | Choose an installer format and whether adb/FFmpeg are bundled or separate prerequisites before packaging. Keep driver elevation separate from normal application execution. |

Platform-independent preparation can proceed while these choices are pending.
Work depending on an unanswered choice must wait for that choice.

## Build environment

- Rust's MSVC target for the selected architecture, Microsoft C++ Build Tools
  and the Windows SDK.
- Windows adb and FFmpeg, with actual executable and encoder discovery.
- Windows CI for compilation, automated tests and artifacts, alongside the
  existing Linux and Android jobs.
- The Windows Driver Kit if building or modifying a driver. Use compatible
  Visual Studio/WDK versions and matching SDK/WDK build numbers from
  [Microsoft's WDK guidance](https://learn.microsoft.com/en-us/windows-hardware/drivers/download-the-wdk).
- An owned driver also needs an appropriate signing and distribution process.
  Microsoft's current Hardware Dev Center submission requirements include
  an EV certificate associated with the account. Verify the applicable
  release route before promising customer installation; see
  [driver signing requirements](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs).

## Architecture and reuse

Retain the Android app and authenticated TCP/WebSocket protocol. Reuse
configuration serialization, version parsing, latency accounting, stream
framing and applicable GUI code. Preserve existing client compatibility,
including the Android brightness and refresh preferences.

Separate OS responsibilities behind focused interfaces: `VirtualDisplay`
for owned monitor lifetime/configuration, `FrameSource` for capture,
`InputSink` for injection, and session/process services for lifecycle and
private runtime state. Platform capabilities should drive GUI and diagnostic
availability. Keep Linux behavior covered throughout the extraction.

| Current implementation | Sources | Windows replacement |
|---|---|---|
| EVDI discovery, EDID attachment and capture helper | `host/src/vdisplay.rs`, `host/src/capture.rs`, `host/src/edid.rs`, `host/evdi/` | Integrate a virtual monitor through an IDD and capture its output. Scope creation and removal to UScreen-owned resources. |
| Raw NV12 through a POSIX FIFO | `host/src/capture.rs`, `host/src/encoder_io.rs`, `host/src/encoder.rs` | Introduce a frame-transfer interface with a Windows pipe or in-process implementation, explicit frame boundaries and cancellation. The existing in-process encoder still has a Unix FIFO input that needs porting. |
| uinput, KWin and X11 mapping | `host/src/input.rs`, `host/src/kwin.rs`, `host/src/kscreen.rs`, `host/src/osk.rs` | Windows pointer injection, display placement and monitor/DPI mapping. Evaluate on-screen keyboard behavior separately. |
| Unix signals, `/proc`, UID checks and file permissions | `host/src/main.rs`, `common/src/linux/mod.rs`, `common/src/linux/runtime.rs`, `host/src/runtime.rs`, `host/src/stream.rs` | Windows process handles/identity, controlled shutdown, per-user single-instance handling, private paths/ACLs, secure randomness and portable socket handling. |
| systemd, D-Bus tray and Linux setup/diagnostics | `gui/src/main.rs`, `host/src/tray.rs`, `host/src/doctor.rs` | Windows lifecycle, tray, optional autostart and diagnostics; normal operation in the interactive user session. |
| VAAPI defaults and Linux build/packaging | `common/src/model.rs`, `host/src/capture.rs`, `host/src/encoder.rs`, `Makefile`, `scripts/`, `packaging/`, `.github/workflows/` | Windows encoder capability detection, build scripts, tests and installation artifacts. |

The [Indirect Display Driver model](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview)
supports virtual monitors and DirectX desktop surfaces. An external driver
plus [Desktop Duplication](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api)
is the initial capture candidate; validate capture of that driver's outputs
before selecting it. Direct IDD swap-chain access would require a suitable
driver interface or an owned driver.

## Milestones and acceptance checks

### 1. Isolate platform dependencies

- Move Linux native calls, imports and dependencies behind platform modules
  and Cargo target conditions.
- Separate protocol handling from input injection and orchestration from
  display/capture operations; share compatible implementations.
- Separate pure tests from OS fixtures. Retain all Linux regressions in the
  Linux suite, and preserve resource privacy and ownership guarantees.

Acceptance: normal Linux tests still pass; shared-interface tests pass;
unsupported capabilities return explicit results rather than false success.

### 2. Produce a Windows build with working lifecycle

- Build daemon and GUI for the confirmed Windows target.
- Implement configuration/runtime paths, private token storage, executable
  discovery, single-instance handling and start/stop/status.
- Port child cleanup, socket handling and GUI actions. Add Windows CI and
  Windows build/test entry points without Linux shell assumptions.

Acceptance: a clean Windows environment builds both executables; the GUI
launches; configuration round-trips; lifecycle commands agree and retire
owned children. Test Unicode paths and spaces. Report missing display/input
capabilities accurately, and label artifacts as incomplete until validated.

### 3. Support pen-only operation

- Implement touch and pen through the Windows
  [synthetic pointer API](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createsyntheticpointerdevice),
  and mouse actions through the appropriate Windows API.
- Map the selected monitor, including negative desktop coordinates, rotation
  and mixed DPI. Preserve pressure, tilt, hover, eraser, button state,
  contact lifetime and the enabled-device policy.
- Test session and privilege restrictions, including ordinary versus elevated
  target applications; expose restrictions accurately.

Acceptance: the existing tablet connects over adb and draws on a selected
Windows monitor; disconnect/reconnect leaves no held contacts or buttons.
This milestone does not require a virtual-display driver.

### 4. Support the extended display and stream

- Detect the chosen driver/version and diagnose missing/incompatible setups.
- Create or claim a UScreen output through the validated interface. Configure
  resolution, refresh and placement without changing other applications'
  virtual monitors.
- Capture, convert and transfer frames to the encoder. Start with the FFmpeg
  process and a working software-encoder fallback.
- Handle mode changes, capture access loss, display removal and reconnects.

Acceptance: Windows recognizes an extended desktop on the tablet; windows
can move onto it and receive mapped input. Automated frame/protocol tests
pass. Hardware checks cover attach/detach, resolution changes, sleep/resume,
desktop locking and repeated reconnects. Cleanup removes only owned resources.

### 5. Validate acceleration and package a release

- Detect and test NVENC, AMF, QSV and CPU encoder candidates on Windows.
  Expose only available, validated choices.
- Measure latency, frame pacing, CPU/GPU use and recovery. Direct GPU-texture
  encoding is a separate optimization requiring implementation and measurement;
  the current raw-frame encoder path does not establish zero-copy operation.
- Finish tray integration, diagnostics, optional autostart, dependencies,
  installer/upgrade/uninstall behavior and distribution notices.
- Document driver setup, supported versions, limitations and recovery.

Acceptance: fresh installation and normal-user operation work on the supported
configuration; upgrades preserve settings; uninstall leaves unrelated drivers
and monitors alone. Publish the tested OS/GPU/tablet matrix, distinguishing
compilation coverage from hardware validation before claiming support.

## Implementation discipline

- Record findings and contradictions in `TODO.md` using its existing format
  and unique IDs; retain decision-dependent entries until resolved.
- Every confirmed behavioral bug needs a permanent automated regression added
  before its fix, demonstrated failing, then passing. If automation is blocked,
  record the exact obstacle and missing coverage and keep the bug unresolved.
  Hardware checks supplement automated coverage.
- Make one commit per resolved finding and remove only its resolved TODO row.
- Keep cyclomatic complexity at or below 9 and responsibilities separated.
- Continue Linux and Android validation throughout the port. Existing blocked
  findings remain tracked separately; saving this plan does not resolve them.
