# Native tablet webcams — T539

Historical implementation record. T543 subsequently moved camera configuration
and session ownership into the host GUI; see the
[host integration validation](2026-09-20-camera-host.md) for current behavior.

The Linux foreground command `uscreen cameras` and the Android **Camera sharing**
controls implement two OS webcam entries with one active tablet lens. The native
Camera2/MediaCodec uplink is separate from the display protocol. See
[setup and limits](../cameras.md). Selecting a desktop camera does not select the
tablet lens; the tablet retains explicit Off/Front/Rear control.

## Device and browser evidence

The RugKing Pad 2 Pro, Android 16, has rear camera ID `0` and front ID `1`.
Linux exposed `/dev/video20` as **UScreen Front** and `/dev/video21` as
**UScreen Rear** using v4l2loopback 0.15.3 with `exclusive_caps=1`.
The signed fork APK was updated in place, retaining its application ID,
certificate and preferences. The existing desktop AppImage was not replaced;
the checkout's `target/debug/uscreen cameras` binary supplied the sidecar.

An isolated Google Chrome 153.0.8010.52 session on localhost granted camera
permission, enumerated both names and opened each exact device ID with the real
`getUserMedia` API. The test used real V4L2 devices, with no fake video source,
microphone capture, account login or meeting creation. Both streams reported
1280x720. In-memory canvas statistics distinguished camera frames from black;
no image or video was retained. See
[browser observations](artifacts/2026-09-20-tablet-cameras/browser.json).

The sequence selected Front, switched to Rear, selected Off, restarted Front,
backgrounded UScreen, then returned to it. Only the selected endpoint contained
camera frames. Both endpoints became black on Off/background, and resuming the
app kept sharing off. Restarting the sidecar required a fresh selection and
successfully captured again. Stopping it during rear capture released camera
ownership; Android reported no active camera clients. Only its ephemeral ADB
reverse mapping disappeared; display mappings `8890` and `8891` remained.
The display service retained PID `68365` and stayed active throughout validation.
No EVDI attachment was initiated.

## Automated checks

- The normal Android suite passed **502 tests** on Robolectric API 27/34, and
  Android lint passed. JaCoCo 0.8.15 measured **548/548 maintained Android
  functions/methods at or above 80% executable-line coverage**.
- Normal Rust workspace tests ran with default and all features. The combined
  Linux report measured **1046/1046 maintained Rust functions at or above 80%**.
  Actual V4L2 startup/shutdown supplemented automated counters for the native
  device-opening and command-entry adapters. The camera-specific report covers
  25 functions. The unrelated GUI publication adapter gap found by the full
  report was covered by permanent isolated T541 tests in a separate commit.
- The repository cyclomatic-complexity gate and Rust formatting/clippy checks
  passed. Existing regressions remain in their normal suites.

Permanent T539 tests cover invalid profiles and bounded packet/header mutation,
authentication, missing permission, explicit consent, foreground lifetime,
switching, late Camera2 callbacks after cancellation, unsupported camera modes,
real H.264 decoding, oversized pictures, stalled decoder input, producer failure,
black output retirement and ownership-safe ADB mapping cleanup.

Regressions first reproduced missing optional camera declarations, final-frame
loss from killing the output producer immediately, an oversized decoded picture,
a blocked decoder pipe and valid pictures rejected by an overly tight decoder
pixel limit. Their retained tests pass after the fixes. FFmpeg validates aligned
decoder buffers as well as visible dimensions, so its bounded pixel allowance
includes alignment padding ([upstream buffer implementation](https://github.com/FFmpeg/FFmpeg/blob/master/libavcodec/get_buffer.c)).
The capture loop explicitly terminates through cancellation/error; its `Nothing`
return type reflects that contract. Camera/session cancellation tests verify
late resources close exactly once. Activity-based Robolectric fixtures isolate
Compose's dispatcher from unrelated reset loopers; assertions were retained.

Evidence in the [artifact directory](artifacts/2026-09-20-tablet-cameras) includes
source hashes, native coverage reports, raw counters, test logs, APK/binary hashes
and browser statistics. Temporary local RPM and FFmpeg development packages
supplied missing test/build prerequisites without modifying system packages.

## Boundaries

This establishes OS webcam enumeration and real Chrome capture, not a completed
Google Meet call. Actual Meet calls, other tablets, sustained battery/thermal
behavior, latency and physical USB unplug/replug were not measured. T540 retains
the physical USB check because removing the connected tablet would also detach
the existing EVDI display through unresolved T222. Windows camera output remains
unimplemented; 58 Windows-only functions remain unmeasured under T493/T497 and
are preserved in the all-platform report. No Windows coverage pass is claimed.
