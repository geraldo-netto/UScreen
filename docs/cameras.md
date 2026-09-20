# Tablet webcams on Linux

The experimental `uscreen cameras` command exposes two ordinary Linux webcams:
**UScreen Front** and **UScreen Rear**. Select either in a browser/video-call
camera picker. Only one tablet camera streams at a time; select Front or Rear in
the tablet's UScreen settings. The other desktop camera produces black video.
Desktop camera selection alone does not switch the tablet lens.

Camera sharing is off by default. An invitation from the desktop never starts
capture. Android asks for camera permission on first selection. Leaving UScreen,
selecting Off, losing the connection or stopping the command releases capture.
This first version is foreground-only: keep UScreen visible during a call.
Camera sharing does not include microphone audio.

## Prepare the devices

Install your distribution's `v4l2loopback` module package, ADB and FFmpeg. Module
installation/loading may require administrator access and Secure Boot signing.
Load two dedicated devices before starting the command:

```sh
sudo modprobe v4l2loopback devices=2 video_nr=20,21 \
  card_label="UScreen Front,UScreen Rear" exclusive_caps=1,1
```

If the module is already loaded with different options, close its consumers
first, unload it with `sudo modprobe -r v4l2loopback`, then run the load command.
Do not unload devices another application is using. The command checks labels
and takes ownership locks; it does not relabel or replace unrelated webcams.
Your user must have read/write access to these nodes, normally through the
desktop's device-access policy or the `video` group.

Use the updated Android APK built from the same checkout, preserving the
[release signing identity](release-signing.md). Open UScreen on the tablet and
connect it through authorized ADB. Then run:

```sh
uscreen cameras
```

For an unreleased source checkout, build with `cargo build -p uscreen --bin
uscreen` and run `./target/debug/uscreen cameras` from the repository root. An
older installed AppImage does not gain the command when the checkout is built.

In tablet settings, find **Camera sharing**, select Front or Rear and grant
camera permission. In Google Meet or another application, select the matching
UScreen webcam. Both endpoints have producers while the command runs so they
can appear in browser camera pickers; the inactive endpoint stays black.
Refresh/reopen a camera picker if it cached devices before producers started.

The command runs in the foreground. Ctrl-C or SIGTERM stops its producers,
clears output and removes only its own temporary ADB reverse mapping. It neither
starts nor restarts the display daemon, and it does not attach EVDI. A fresh
invitation or restarting the Android app requires explicit camera selection
again. After a capture error, select the desired camera again to retry.

## Profile controls

```sh
uscreen cameras --serial TABLET_SERIAL --width 1280 --height 720 \
  --fps 30 --bitrate 3000 --front-device /dev/video20 --rear-device /dev/video21
```

Default capture is 1280x720, 30 FPS and a 3000 kbit/s H.264 target. Optional
`--mirror` mirrors exported video. Width accepts even values 160–1920; height
accepts even values 120–1080; FPS accepts 5–30 and bitrate accepts 256–20000
kbit/s. These are request limits, not guaranteed modes: Android rejects profiles
its camera does not advertise. A bitrate target is not a measured traffic rate.
Larger profiles cost USB bandwidth, decoding/encoding work and tablet power.

The desktop uses one persistent FFmpeg producer per virtual camera and an
FFmpeg decoder for the selected camera. Android uses Camera2 and MediaCodec.
A dedicated authenticated local TCP connection crosses a temporary ADB reverse
mapping, independent of the display and pen connections. Its `USCAM001` header
contains the session credential, lens identity and rotation, followed by bounded
big-endian length-prefixed H.264 packets. Packets are capped at 2 MiB; connection,
read/write and encoder-progress deadlines bound stalled sessions. Each new lens
connection gets a new decoder; no encoded history crosses the switch.
FFmpeg's decoder pixel budget includes alignment padding; individual allocations
are capped at 64 MiB. Output frames always use the selected desktop dimensions.

## Validation status

On 2026-09-20, the connected RugKing Pad 2 Pro (Android 16) delivered both rear
ID `0` and front ID `1` through Chrome 153's real `getUserMedia` API at 1280x720.
Chrome enumerated **UScreen Front** and **UScreen Rear** as separate devices.
Switching, Off, background/resume, restarting capture and restarting the desktop
sidecar were exercised. The inactive device was black, returning to the app
left sharing off, and stopping the sidecar released both Android cameras while
preserving the display's ADB mappings and running service.

Automated T539 tests cover permissions, profile and packet bounds, consent,
foreground lifetime, switching, late callbacks after cancellation, real H.264
decoding, stalled/oversized input, output cleanup and mapping ownership. See the
[implementation validation](reviews/2026-09-20-camera-implementation.md) for
test/coverage evidence and limits. An actual Google Meet call, physical USB
unplug/replug, other tablets and sustained power/latency were not validated.
No camera images were retained in the evidence. The original
[feasibility assessment](reviews/2026-09-20-tablet-cameras.md) preserves the
initial hardware observations. Windows webcam output is not implemented.
