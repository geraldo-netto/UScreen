# Tablet cameras as desktop cameras

Assessment: 2026-09-20. Follow-up: T539. This is a feasibility assessment and
proposed design, not an implemented UScreen capability.

The maintainer wants separate front/rear desktop camera entries and confirmed
that only one physical camera needs to operate at a time. This is technically
feasible on the current Linux host. Device inspection supports a prototype;
working camera video, application compatibility and performance remain untested.

## Evidence from this checkout and connected hardware

[Retained capability observations](artifacts/2026-09-20-tablet-cameras/capabilities.json)
include the checkout revision, commands and selected camera metadata. Inspection
did not open either camera, record images, load a driver or change USB functions.

| Component | Observed state | Limit of the evidence |
| --- | --- | --- |
| RugKing Pad 2 Pro | Android 16, API 36; two public cameras | HAL internal entries are not additional public cameras |
| Rear camera | Public ID `0`, sensor orientation 90 degrees | Enumerated, not opened |
| Front camera | Public ID `1`, sensor orientation 270 degrees | Enumerated, not opened |
| Camera formats | Both advertise 1280x720 and 1920x1080 PRIVATE/YUV outputs, plus a 30–30 AE FPS range | Separate metadata does not prove a working encoded mode or sustained 30 FPS |
| Linux host | Kernel `7.0.0-31-generic`; `v4l2loopback` 0.15.3 module present | Module unloaded; no `/dev/video*` nodes at inspection |
| Prototype tools | ADB and FFmpeg available; `scrcpy`, `v4l2-ctl`, `v4l2loopback-ctl` absent from PATH | No desktop capture test performed |
| Native Android webcam | `com.android.DeviceAsWebcam` installed; `ro.usb.uvc.enabled` empty | Package presence does not establish an enabled USB webcam function |

Current implementation has no camera capture/export path:

- [`AndroidManifest.xml`](../../android/app/src/main/AndroidManifest.xml) declares
  no `CAMERA` permission; the foreground service type is `connectedDevice`.
- [`VideoTransport.kt`](../../android/app/src/main/java/com/uscreen/VideoTransport.kt)
  receives host video, and
  [`StreamingService.kt`](../../android/app/src/main/java/com/uscreen/StreamingService.kt)
  manages the existing notification and power locks.
- [`host/src/linux_main.rs`](../../host/src/linux_main.rs) establishes ADB reverse
  mappings; [`host/src/stream.rs`](../../host/src/stream.rs) sends display video,
  while [`host/src/input.rs`](../../host/src/input.rs) receives authenticated
  input/control. No camera encoder, camera uplink or V4L2 output exists.
- Android minimum API is 27 in
  [`build.gradle.kts`](../../android/app/build.gradle.kts). The native app route
  can use Camera2 within that baseline; the scrcpy prototype below requires
  Android 12 or newer.

## Available approaches

**UScreen integration: recommended for two stable named entries.** Use Android
Camera2 capture, a MediaCodec H.264 encoder, a dedicated authenticated return
stream through ADB, host decoding and two Linux virtual-camera endpoints.
Camera2 supports camera enumeration/opening; MediaCodec accepts encoder input
through a Surface. These APIs support the design, not a throughput guarantee.
[CameraManager](https://developer.android.com/reference/android/hardware/camera2/CameraManager),
[MediaCodec](https://developer.android.com/reference/android/media/MediaCodec#createInputSurface()).

**scrcpy: smallest independent proof of the media path.** Upstream supports
front/back camera capture on Android 12+ and a Linux V4L2 sink. After preparing
one owned virtual-camera node, test each camera sequentially with audio disabled.
This verifies a component path without adding capture to UScreen; it does not
provide UScreen's two-entry switching/lifecycle integration by itself.
[Camera capture](https://github.com/Genymobile/scrcpy/blob/master/doc/camera.md),
[V4L2 output](https://github.com/Genymobile/scrcpy/blob/master/doc/v4l2.md).

**Android's built-in USB webcam: potentially useful, not established here.** AOSP
supports it from Android 14 QPR1, with a front/back selector for the exported
feed. It needs OEM USB gadget/HAL configuration and an enabled build property.
This does not establish two independently named outputs, nor coexistence with
UScreen's current USB connection. The installed package alone is insufficient;
do not change USB functions during an active display session to infer support.
[AOSP webcam implementation](https://source.android.com/docs/core/camera/webcam).

## Proposed UScreen behavior

1. Expose `UScreen Front` and `UScreen Rear`, with identities bound to tablet and
   lens facing. Discover camera IDs; do not assume every tablet uses `0` and `1`.
2. Keep capture off by default. Start the selected camera following tablet
   permission/consent and stop it when disabled, disconnected or permission is
   revoked. For background capture, add the camera foreground-service permission
   and type, and start under Android's while-in-use restrictions. Existing
   display-only operation must remain usable without camera permission.
   [Camera foreground-service rules](https://developer.android.com/develop/background-work/services/fgs/service-types#camera).
3. Encode one camera at a time. Use a separate bounded media channel so camera
   backpressure does not queue behind pen/control messages. Reuse attachment
   identity/authentication policy, with explicit protocol capability negotiation,
   camera identity, generation, codec configuration, packet bounds and recovery.
4. Decode on the host and feed the corresponding virtual endpoint. On switch,
   stop/close the previous capture session, retire its encoder/decoder generation,
   clear stale output and then open the requested camera. Never route rear frames
   into an endpoint named Front. Normalize rotation and make mirroring explicit.
5. Keep the inactive endpoint visibly inactive. Linux `v4l2loopback` supports
   multiple named devices. With `exclusive_caps=1`, an endpoint advertises capture
   only while a producer is attached. Keeping both entries selectable therefore
   needs an attached placeholder producer or another verified compatibility
   approach. Use blank/inactive output rather than a retained camera image.
   [v4l2loopback options](https://github.com/v4l2loopback/v4l2loopback#options).
6. Initially, an explicit UScreen camera selector is the simplest control.
   Automatically activating whichever endpoint a desktop app opens needs separate
   consumer-demand tracking and arbitration. Desktop apps may probe both entries;
   that must not repeatedly flip cameras. Reject or show inactive status for a
   competing request; do not silently take over an active session. Validate this
   before promising that desktop camera selection alone controls tablet capture.

The Linux backend needs module installation/loading and suitable device access.
Host decoding/output is separate from EVDI and should not require display
reattachment. The ADB/app route does not require rooting the tablet or modifying
its firmware. Windows would need its own virtual-camera backend; current
[Windows host preview](../windows-port.md) does not supply one.

## Capacity and acceptance

Suggested starting profile, not a measured optimum: 1280x720 at 30 FPS, H.264
around 3 Mbit/s, with user controls for resolution, FPS, bitrate and mirroring.
Allow 1080p only after negotiating a working camera/encoder mode. Encoding one
camera while decoding UScreen display video adds resource demand; combined USB
traffic, thermal behavior, battery use and frame pacing need measurement on the
existing tablet. No additional-tablet campaign is required.

Implementation is a moderate feature across Android, protocol, Linux output,
settings and packaging, not a configuration-only change. Before enabling it:

- Verify both physical cameras sequentially, correct orientation, repeatable
  switching and normal desktop/browser camera enumeration.
- Verify display/pen coexistence using the existing session without reattaching
  EVDI; exercise disconnect/reconnect, camera busy, permission denial/revocation,
  background/foreground transitions and process failure.
- Retain automated tests for stable identity, single-camera ownership, stale
  callbacks/frames across switching, bounded malformed/truncated input, stalled
  consumers, failure cleanup, inactive output and settings persistence. Apply
  the repository's per-function coverage and bounded fuzz requirements. Any
  demonstrated behavioral bug requires a permanent failing test before its fix.
- Measure actual capture-to-desktop latency, CPU/thermal/battery impact and
  supported formats. Advertised metadata and installed dependencies are not
  passing runtime or performance results.

T539 keeps this prototype and integration evidence open. No behavioral bug was
fixed during the assessment; no production code or automated tests changed.
