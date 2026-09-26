# Windows integration plan

Status: staged implementation, updated 2026-09-26. Linux remains the supported
runtime. Windows capture, input, lifecycle and packaging are not implemented.
The first Windows release target is **Windows 11 x64**, selected by the maintainer
on 2026-09-26. Windows 10 and ARM64 are outside this initial scope. This target
decision does not enable runtime capabilities; driver and packaging choices
remain pending.

The initial [cross-compilation review](reviews/2026-09-19-windows-cross-compilation.md)
recorded GNU/MSVC failures at `63b332e`. The subsequent
[T494 daemon boundary](reviews/2026-09-19-windows-cli.md) builds a Windows GNU
command-line diagnostics executable and passes MSVC target checking. Help and
version work; runtime commands return explicit unsupported errors. This is a
compilation preview, not a functioning second-screen server or a selected
release toolchain.

The subsequent [shared-service work](reviews/2026-09-19-windows-services.md)
adds Windows paths, executable discovery, process jobs and private-state
primitives. T493 remains blocked on native ACL/lease validation; these
primitives do not enable a Windows application backend.

The [T495 GUI boundary](reviews/2026-09-19-windows-gui.md) also builds for GNU
and checks on MSVC. Its Windows preview saves shared settings and hides Linux
setup/capacity controls; unavailable lifecycle actions fail explicitly. Automated
headless UI tests and a real window launch passed under Wine, not native Windows.

T496 adds [native MSVC tests and GNU cross-build CI](../.github/workflows/windows.yml).
Shell-free command fixtures run as Windows executables; Linux process-group
assertions remain in the Linux suite. Local policy and library execution passed
under Wine. The native CI workflow is configured but has not been run from this
checkout; its mandatory ACL/lease tests must pass before T493 can close.

T533 shares dependency diagnostics between `blent doctor` and the GUI status
worker. ADB and FFmpeg report the discovered executable path and parsed version,
or distinguish a missing program from an unverified failed/malformed check.
Version commands use the platform process adapter with a two-second deadline
per dependency. GUI results share the existing ten-second capability cache;
the UI does not run commands while rendering. Backend flags report unsupported
or implemented-but-unverified operations. Discovery never establishes tablet
connection, driver initialization or encoder compatibility, and `doctor` still
exits unsuccessfully while the Windows application backends are unavailable.

Delivery sequence: **Windows compilation → pen-only operation → extended
display → packaged release**. Each milestone has separate acceptance checks;
a successful Windows build alone does not establish functional support.

## Decisions and resources needed

| Decision or resource | Recommendation | Condition to proceed |
|---|---|---|
| Windows target | Windows 11 x64, accepted 2026-09-26 | Development VM uses the official 90-day Enterprise evaluation. Windows 10 and ARM64 are outside the first release; native acceptance remains required. |
| Virtual-display driver | Integrate an existing [Virtual Display Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver) installation | Confirm that a separately installed driver is acceptable. Pin and validate its version and control interface before implementing display integration. |
| Hardware testing | A Windows PC connected to the Android tablet | Provide a test machine or tester, its Windows version and GPU, and administrator access for driver installation when authorized. CI or a VM can cover builds and isolated tests; USB, GPU and tablet validation need physical hardware. |
| Owned driver, if required | Treat driver ownership as a separate deliverable | Confirm maintenance, signing and distribution responsibilities before developing or shipping our own driver. |
| Application distribution | Per-user application with optional autostart | Choose an installer format and whether adb/FFmpeg are bundled or separate prerequisites before packaging. Keep driver elevation separate from normal application execution. |

Platform-independent preparation can proceed while these choices are pending.
Work depending on an unanswered choice must wait for that choice.

## Build environment

- Rust's `x86_64-pc-windows-msvc` target, Microsoft C++ Build Tools and the
  Windows SDK for the accepted Windows 11 x64 target. GNU cross-builds remain
  portability checks; they do not widen the initial release scope.
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

The Android app and authenticated TCP/WebSocket protocol remain unchanged.
T523 exposes attachment ownership, credentials, control/video transport, bounded
queues, latency accounting, selection data and session orchestration through the
portable host library. Linux uses that same implementation. Real loopback-socket
tests cover authentication, wire bytes, render acknowledgements, attachment
replacement, failed startup and worker retirement using injected adapters.

`session::CaptureBackend` supplies shared capture resources and returns every
owned native worker after both listeners bind. `input::backend::InputBackend`
and `InputSink` separate controller handling from injection and native display
mapping. Linux owns its EVDI card receiver inside the input adapter. Shared
credentials use stock `getrandom`; socket buffer hints use stock `socket2`.
Private credential persistence and process lifecycle remain platform services.

These interfaces compile for GNU and MSVC Windows targets, but the Windows
preview does not start a session or enable backend capabilities. Windows native
display/capture/input adapters still need implementation and acceptance.
Narrower `VirtualDisplay` and `FrameSource` interfaces remain design candidates
for those adapters, not implemented cross-platform backends. Preserve existing
Android brightness/refresh preferences and Linux regressions during that work.

| Current implementation | Sources | Windows replacement |
|---|---|---|
| EVDI discovery, EDID attachment and capture helper | `host/src/vdisplay.rs`, `host/src/capture.rs`, `host/src/edid.rs`, `host/evdi/` | Integrate a virtual monitor through an IDD and capture its output. Scope creation and removal to Blent-owned resources. |
| Raw NV12 through native transfer adapters | `common/src/raw_frame.rs`, `host/src/encoder_shared.rs`, `host/src/raw_memory.rs`, `host/src/encoder_io.rs` | FIFO and the optional Linux sealed-memfd adapter implement input ownership and cancellation. Reuse the portable bounded descriptor contract and final-reference lease semantics; supply a Windows mapping/handle-transfer or framed pipe adapter. Linux memfd, Unix sockets and eventfd are not Windows implementations. |
| uinput, KWin and X11 mapping | `host/src/input/linux.rs`, `host/src/input/mapping.rs`, `host/src/input/event_writer.rs`, `host/src/kwin.rs`, `host/src/kscreen.rs`, `host/src/osk.rs` | Windows pointer injection behind `InputBackend`/`InputSink`, display placement and monitor/DPI mapping. Evaluate on-screen keyboard behavior separately. |
| Unix signals, `/proc`, UID checks and file permissions | `host/src/linux_main.rs`, `common/src/linux/mod.rs`, `common/src/linux/runtime.rs`, `host/src/runtime.rs` | Windows process handles/identity, controlled shutdown, per-user single-instance handling and private paths/ACLs. Shared transport no longer needs Unix socket calls or a Linux credential source. |
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
- Create or claim a Blent output through the validated interface. Configure
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
