# Architecture

This describes the Linux host and Android client in the current checkout.
Windows remains a [proposed integration](windows-port.md). Known behavioral
limits are tracked in [TODO.md](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/TODO.md); describing a path does not certify
it on every desktop or device.

The [2026-09-17 architecture review](reviews/2026-09-17-architecture.md) records
merge/split opportunities, ownership boundaries and required regression coverage.
Its [performance research](reviews/2026-09-17-performance-scalability.md) proposes
Android and Rust/C measurements and improvements; these are not benchmark results.

## Pipeline

1. **Virtual display.** A C helper opens an EVDI device and presents a generated
   EDID for the selected resolution and physical size. The desktop compositor
   supplies frames. Automatic placement uses `kscreen-doctor`; calls are not
   fully restricted to supported desktops yet (T224).
2. **Capture.** The helper requests updates, grabs BGRA pixels, converts damaged
   rows to NV12 (BT.709, limited range) and sends raw frames through a FIFO in
   the runtime directory. FIFO writes can be partial; correct recovery from an
   interrupted partial frame remains unresolved (T226). The target FPS is not
   a guarantee of capture throughput.
3. **Encode.** The default FFmpeg child uses NVENC, VAAPI or software libx264.
   NVENC uses VBR/constant-quality targeting, VAAPI uses CQP, and libx264 uses
   CRF with VBV limits. The configured bitrate is not a VAAPI ceiling (T259).
   B-frames/lookahead are disabled on the low-latency paths. An optional
   in-process libavcodec encoder avoids the child process; it does not support
   `ten_bit`, and its VAAPI initialization is incomplete (T284). VAAPI needs
   the default FFmpeg path until hardware-frame integration is implemented.
4. **Keyframes and delivery.** The FFmpeg CLI requests an IDR each second of
   capture wall-clock time, including the five-fps idle floor. Actual recovery
   also waits for capture, encoding, packetization and transport. The software
   regression checks recovery under 1.6 seconds at idle. The optional in-process
   encoder can honor a next-frame keyframe request; its periodic GOP counts
   frames. The TCP server skips a lagging client's backlog to a retained IDR,
   or waits for another IDR if necessary.
5. **Decode.** Android receives length-prefixed Annex B access units through
   adb forwarding and feeds MediaCodec, rendering to a SurfaceView. It requests
   low-latency hints where supported. MediaCodec selection does not guarantee a
   hardware decoder; codec/profile/resolution support is device-dependent.
6. **Input.** A WebSocket carries touch, pen and control messages back to the
   host. Enabled uinput devices exist while a tablet is attached: touchscreen,
   pressure/tilt/eraser/button pen, and an absolute pointer used with the pen.
   `input_touch`, `input_pen` and `input_pointer` control creation; the pointer
   also requires the pen. KDE Wayland mapping uses KWin D-Bus; X11 uses
   `xinput`/`xrandr`. Other desktops need their own mapping facilities.
7. **Latency loop.** Every video frame has a sequence number, echoed after the
   Android render callback. Host p50/p95 measure encoded-packet readiness to
   acknowledgement receipt, including the return path and excluding capture,
   encoding and packetizer assembly. See [measurement boundaries](benchmarks.md#how-latency-is-measured).

## Processes and settings

`uscreen-config::model` owns the portable settings schema, sanitization and edit
merging. Its `storage` adapter owns transactional files; `commands` owns bounded
process execution; `linux` owns Linux process/runtime state. Default features
retain the existing Linux API, while `--no-default-features` builds policy and
version comparison without filesystem/process adapters. CI checks that boundary
on WebAssembly; this does not make the daemon or GUI Windows-compatible.

- `uscreen`: daemon, adb monitor, per-tablet sessions, tray and settings state.
- `evdi_helper`: one per active display slot, owns one EVDI card. The daemon's
  card assignment can pin an occupied card despite free capacity (T330).
- `ffmpeg`: one per active encoding slot, unless built with the optional
  in-process encoder.
- `uscreen-gui`: host configuration and start/stop controls. Apply & Restart
  runs one background save and restarts only after successful persistence.
  Controls remain editable during a save; completion preserves those newer
  edits and merges unrelated disk changes into the saved baseline. Save,
  Discard and daemon actions wait for that operation to finish. A save failure
  retains edits for retry; a restart failure still leaves the saved baseline
  current. Closing the process can interrupt a pending job; atomic replacement
  preserves either the old or new config, not a partial file.
  Encoder/display edits on disk generally require a daemon
  restart; tablet control messages can update live settings. The Wi-Fi
  reconnect address is reread from disk for each attempt.

The Android foreground service follows the Activity's started lifecycle,
including waiting for connection and graphics-tablet mode; it is not proof
that video is currently being decoded. Window brightness and preferred display
mode are app-local controls, separate from host stream FPS and encoding.

Daemon settings and mode persistence share one filesystem worker with one
waiting queue slot. Their watch channels coalesce subsequent updates while a
transaction is pending. Successful completion advances the settings baseline;
failed edits remain pending for the next update. Shutdown cancels lock waits
and joins the worker, including any already-started filesystem commit. Neither
aborting an async caller nor dropping a handle rolls back an in-progress write.

`host/src/kscreen.rs` owns the KScreen command boundary and typed output
inventory. Placement derives logical bounds from pixels and scale; input
mapping chooses a connector; diagnostics report raw mode dimensions and color
profiles. They share parsing without conflating these policies. Missing fields
retain the established defaults, and a missing connector name remains distinct
from an empty name. Graphics-tablet mapping prefers an enabled physical output
marked primary by either the legacy boolean or modern `priority: 1` schema,
then falls back to the first enabled physical output in inventory order.

## Protocol

Slot indices start at zero. Default video port is `8890 + 2*slot`; default
input port is `8891 + 2*slot`. Both listeners bind to `127.0.0.1`; configured
base ports must leave non-overlapping valid ports for every slot.

With default token authentication enabled, both connections must present the
same per-run 64-character hex token. Video authentication has a three-second
deadline; WebSocket upgrade and input authentication share a three-second
accept-to-authentication deadline. Disabling token checks changes the wire
handshake and is incompatible with the current Android video client (T267).

### Video TCP

The client sends exactly 64 ASCII token bytes, with **no newline or length
prefix**. After authentication, the server sends:

| Field | Encoding |
| --- | --- |
| Packet length | Four-byte unsigned big-endian integer; excludes these four bytes, includes type and all payload bytes |
| Packet type | One byte: 0 for codec configuration, 1 for frame |
| Type 0 payload | Annex B codec parameter sets: SPS/PPS for H.264, VPS/SPS/PPS for HEVC |
| Type 1 payload | Four-byte unsigned big-endian sequence number, then Annex B access-unit bytes |

The codec is announced on the input/control connection; there is no separate
codec-name field in the video packet header.

### Input/control WebSocket

The first message is `{"type":"auth","token":"…"}` when authentication is
required. A representative host greeting is valid JSON:

```json
{"status":"connected","width":2960,"height":1848,"fps":60,"codec":"h264","pen_only":false,"touch":true,"pen":true}
```

The server sends `status: "mode"` for subsequent mode/settings notifications.
`codec` is `h264` or `hevc`; FPS is omitted if no shared settings source exists.
Width/height currently come from startup input configuration and can be stale
after geometry negotiation (T276); they are not reliable current-stream dimensions.
The Android client marks its control connection authenticated only after
`status: "connected"` from the current socket. Disconnects clear that state;
callbacks from replaced sockets cannot restore it. Rust serialization and
Android's pen-mode UI use the same `control-connected.json` regression fixture.

Examples of individual client messages (one JSON object per WebSocket message):

```json
{"type":"touch","x":0.5,"y":0.3,"pressure":1.0,"action":0,"slot":0}
{"type":"pen","x":0.5,"y":0.3,"pressure":0.8,"tilt_x":12.0,"tilt_y":-3.0,"eraser":false,"action":2}
{"type":"resolution","width":2960,"height":1848,"width_mm":314,"height_mm":195}
{"type":"config","bitrate":20000,"fps":60,"encoder":"h264_nvenc"}
{"type":"mode","pen_only":true}
{"type":"rendered","seq":1234,"decode_us":14200}
```

Coordinates are normalized to 0–1; wire tilt values are degrees. Touch actions
are 0 down, 1 up, 2 move; pen adds 3 hover, 4 hover exit, 5/6 stylus-button
down/up. Physical dimensions, eraser and decode timing have defaults when
omitted; config messages may omit settings they do not change. Pen tilt's
uinput axis metadata has a separate unresolved libinput scaling issue (T287).

## Security model

Default authentication, local token storage and loopback binding restrict
access but do not protect against processes with the same-user/adb privileges.
Existing runtime-directory validation is incomplete (T252). USB carries the
stream over the cable; Wi-Fi setup opens the tablet's adb TCP listener and
carries the stream over that connection. See [SECURITY.md](../SECURITY.md) for
trust boundaries, update checks and uninstall behavior.
