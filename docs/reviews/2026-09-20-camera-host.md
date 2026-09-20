# Host camera integration — T543

The desktop Cameras tab owns manual Start/Stop, lens selection, resolution,
frame rate, bitrate, mirroring, additional clockwise rotation and background
capture. Settings persist without restoring capture at launch. Camera-only
changes do not restart the display daemon. The Linux GUI embeds the camera
library; a separate sidecar is no longer needed. See [usage](../cameras.md).

The preview samples the same transformed YUV frames sent to the webcam output,
at up to five thumbnails per second and a maximum dimension of 160 pixels.
It clears on stop or after two seconds without a fresh frame. Android retains
the required permission prompt and a status display; camera settings live on
the host. Background capture uses a camera foreground service and a CPU wake
lock, both retired when capture ends.

## Native evidence

On the connected RugKing Pad 2 Pro running Android 16, the host GUI selected
front ID `1` and rear ID `0`. Chrome 153 opened the real **UScreen Front** and
**UScreen Rear** V4L2 devices at 1280x720. Only the host-selected lens contained
camera frames; the other device was black. Rear capture continued with the
Activity hidden and with Android reporting `mWakefulness=Dozing`. Host Stop
then blackened both outputs and released capture. A separate foreground-only
run stopped producing camera images when the Activity hid.

These observations predate the final preview/rotation additions and the hidden
permission-loss correction. The final rotation pipeline was checked separately
with a real H.264 stream containing asymmetric red/blue regions: all four
rotations produced the expected exported pixels and thumbnail. An egui test
checked texture creation, replacement, reuse and clearing. The final permission
correction passed the full Android suite. No further deliberate lock/sleep
actions were performed after the maintainer warned about manual unlocking.

The initial browser probe incorrectly reused ended tracks after a full Stop;
those two samples do not validate restarted capture. The fresh foreground-only
probe opened new tracks and passed. Both raw observations are retained, with
the applicable phases identified in [validation.json](artifacts/2026-09-20-camera-host/validation.json).
No camera image or video was saved. No Google Meet call was made.

## Automated checks

- Android: **518 tests**, zero failures/errors/skips; lint passed. JaCoCo measured
  **559/559 maintained production functions/methods at or above 80%**.
- Rust: workspace default and all-feature suites passed. Combined LLVM coverage
  measured **1085/1085 Linux production functions at or above 80%**. Native
  V4L2 startup and graceful shutdown supplemented automated coverage of the
  device-opening and command-entry adapters.
- Complexity: **5037 functions**, none above cyclomatic complexity 9.
  Rust formatting and all-feature/all-target clippy passed.
- Windows GNU workspace cross-check and the portable config crate's no-default
  WASM check passed. These checks do not establish native Windows/macOS support;
  58 Windows-only functions remain unmeasured under T493/T497.

Permanent T543 regressions cover manual ownership, no automatic capture/retry,
ordered restart/shutdown, camera-only configuration persistence, host lens
authorization, permission denial/loss, background-service failure, Activity
callback detachment, wake-lock retirement, malformed invitations, rotation,
preview bounds and output/preview consistency. Host lens rejection and hidden
permission-loss tests were run failing before their fixes and passing afterward.
Earlier T539 regressions remain in the normal suites. T542 separately replaced
the retired endpoint's raw connection error with actionable host instructions.

Portable profile/state/preview/backend contracts live in `uscreen-config`.
The host controller owns platform-independent session lifecycle; the GUI
selects the native adapter at one factory. Linux device paths, V4L2 ownership,
process launch and ADB mapping remain in the Linux adapter. Windows and macOS
still require native output adapters and validation.

## Deployment and remaining limits

The final signed APK was installed in place, preserving the application ID,
signing certificate and app data. Linux installation uses the updated AppImage
at the existing launcher path, with the previous image retained for rollback.
The running display service is intentionally preserved: its existing process
continues using its old extraction until the next normal service restart.
The updated GUI and embedded camera library are available immediately.
The packaged GUI opened its Cameras tab successfully in Xvfb. The installed
launcher exposed the new CLI options, started both V4L2 outputs using bundled
ADB/FFmpeg, accepted graceful SIGTERM and removed its temporary mapping. The
installed Android APK matched the signed build's SHA-256 byte-for-byte; package
hashes and rollback details are retained in
[deployment.json](artifacts/2026-09-20-camera-host/deployment.json).

The display service retained its original PID and ADB mappings `8890`/`8891`;
camera shutdown removed only its own mapping. No EVDI attachment was initiated.
Physical USB unplug/replug remains blocked by T222/T540. T549 tracks the exact
Android lock/unlock triggers; the screen-off observation is not a guarantee for
every secure-keyguard or vendor power-management configuration. Other tablets,
actual Meet calls, sustained power/thermal behavior and latency were not measured.

GitHub workflows remain present and accept only manual `workflow_dispatch`.
Future microphone, audio-output and NFC work remains deferred in T544–T546.
