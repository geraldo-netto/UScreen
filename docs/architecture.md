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
   supplies frames. Automatic placement uses `kscreen-doctor` only in a KDE
   Wayland session; other desktops manage placement through their own settings.
2. **Capture.** The helper requests updates, grabs BGRA pixels, converts damaged
   rows to NV12 (BT.709, limited range) and sends raw frames through a FIFO in
   the runtime directory. After a partial write the helper quarantines that
   FIFO inode and reports `FIFO_RESET <device> <inode>`. The host retires the
   encoder reader and replaces the FIFO before encoding resumes. A retained,
   idle writer descriptor prevents EOF from racing ahead of the reset report;
   it closes when the helper opens the replacement FIFO or shuts down. The helper
   and virtual display stay attached; delayed reports for old inodes are
   ignored. The target FPS is not a guarantee of capture throughput.
3. **Encode.** The default FFmpeg child uses NVENC, VAAPI or software libx264.
   NVENC uses VBR/constant-quality targeting, VAAPI uses CQP, and libx264 uses
   CRF with VBV limits. The configured bitrate is not a VAAPI ceiling (T259).
   B-frames/lookahead are disabled on the low-latency paths. An optional
   in-process libavcodec encoder avoids the child process; it does not support
   `ten_bit`. The optional build rejects VAAPI before capture setup and rejects
   live tablet requests to select it; VAAPI uses the default FFmpeg child path.
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
   The output watchdog reconnects after four queued frames and more than 1.5
   seconds without output. Two stalls disable latency hints. Only four output
   frames spanning at least 1.5 seconds, with no gap longer than that window,
   clear the failure streak. Recreating the codec starts a new recovery window;
   once selected, the hint fallback persists for that receiver's lifetime,
   including stop/start and codec changes.
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

The Rust `capture` module supervises settings changes, startup cancellation,
encoder sessions and retry ordering. `capture::helper` owns the helper child,
FIFO creation, preferred-card identity and geometry announcements;
`capture::encoding` owns encoder children/tasks and the in-process cancellation
flag. `capture::cli_encoder` translates policy to FFmpeg arguments and drains its
encoded output. `capture::placement` handles desktop placement, while
`capture::process` provides bounded termination and validated orphan retirement.
The `media` module owns codec, packet, generation and live-settings contracts,
so streaming, input and encoding do not depend on capture management.
`annex_b` assembles access units using `encoder_io`'s shared NAL scanner. An
incremental cursor avoids rescanning retained NAL payloads; complete NALs are
borrowed from the retained input buffer while assembling access units. Codec
headers use immutable shared `Bytes`, preserving each queued packet's original
configuration. Payload buffering/assembly still copies bytes. Partial NAL
buffering and generation retirement remain part of that boundary; the
[packetizer replay](benchmarks.md#annex-b-packetizer-replay) records its measured
allocation, copy and scanning changes. `capture::fifo` coordinates replacement of a damaged
raw-frame FIFO; it creates the replacement before unlinking the old inode,
preventing inode reuse during recovery. Raw frames still have no in-band
sequence, size or generation header; both processes must use this reset
protocol rather than assuming a close/reopen establishes a frame boundary.

`uscreen-config::model` owns the portable settings schema, sanitization and edit
merging. Its `storage` adapter owns transactional files; `commands` owns bounded
process execution; `linux` owns Linux process/runtime state. Default features
retain the existing Linux API, while `--no-default-features` builds policy and
version comparison without filesystem/process adapters. CI checks that boundary
on WebAssembly; this does not make the daemon or GUI Windows-compatible.

- `uscreen`: daemon, adb monitor, per-tablet sessions, tray and settings state.
  `session::Spec` prepares the same settings, capture and control/video servers
  for every slot. Preparation exposes settings before producers start so the
  primary daemon can snapshot persistence and CLI overrides. Both listeners
  bind before any session worker starts. `session::Runtime` owns the capture,
  server, display-gate and shutdown tasks; stopping a slot waits for capture
  cleanup before retiring its remaining tasks. Daemon-wide shutdown reaches
  every slot, while disconnecting an extra tablet stops only that runtime.
  Mode/settings persistence remains a primary-daemon responsibility.
- `evdi_helper`: one per active display slot, leases one free EVDI card.
  Automatic sessions skip connected cards and contend through nonblocking
  exclusive DRM-inode locks. A restarting session prefers its previous card
  when free, then searches other cards; it does not reserve an inactive card.
  Card removal or another application's use can change the assignment.
  The helper publishes the actual card for placement/input mapping. If no card
  is available it tries to add one; failure leaves capture waiting/retrying.
  An explicit helper `--card` pin remains strict and never falls back.
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

Attachment metadata is producer-owned (`attachment.rs`). Before forwarding or
launching a replacement, the monitor advances a control epoch and invalidates
geometry synchronously. A boolean presence watch can therefore coalesce rapid
detach/attach without keeping the previous tablet's geometry. Accepted control
sockets capture that epoch before authentication; retired sockets cannot claim
controller ownership or dispatch metadata/input. Each dispatch is serialized
with epoch invalidation, and retirement cancels a stalled control writer.

A direct USB/Wi-Fi handoff preserves geometry only when discovery proved both
transports belong to the same physical tablet. It still retires the old control
epoch. An observed absence or an unknown/different identity requires new native
and physical dimensions. Metadata received during forwarding setup belongs to
the new epoch and survives a delayed display-gate consumer. Extra slots apply
the same rules through their session runtime; their transport migration still
recreates that slot. Capture waits for current geometry before starting a helper.

The CLI and GUI share the Linux CLI grammar and same-user daemon discovery.
A PID file is a hint: its entry receives priority only after UID, liveness and
full command-line validation. Missing, stale or diagnostic-command PID entries
fall back to process discovery. An active user service routes GUI actions through
systemd; otherwise a live direct daemon takes precedence over an installed
inactive unit. Doctor uses the same daemon validation and one read-only process
inventory. Helpers and encoders are associated by same-user ownership, executable
identity and the configured slot's exact FIFO argument; unrelated capture
processes and concurrent diagnostic commands do not count as orphans. Its
remediation uses validated UScreen stop/start operations rather than broad
process-name signals.

Autostart uses the loaded systemd user unit when available, with an XDG desktop
entry as the fallback. The installer and GUI share the fallback template and
test their generated commands against each other. The GUI persists the login
preference separately from controlling the current daemon; direct start/stop
still follows the same validated daemon-discovery rules.

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
accept-to-authentication deadline. With checks disabled, video accepts either
no token or one saved 64-byte hexadecimal token; neither is authenticated.
This permits existing Android preferences and legacy tokenless clients to work
without changing the enabled-authentication handshake.

### Video TCP

The client sends exactly 64 ASCII token bytes, with **no newline or length
prefix** when authentication is enabled. With authentication disabled, the
server starts sending immediately and concurrently consumes an optional token.
Once that optional prefix begins, its remaining bytes must arrive within three
seconds. Malformed, incomplete or extra client input closes the viewer; EOF
releases its capture subscription even when no video frames are available.
The server sends:

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

Android's `TouchCapture` is the Activity-facing facade. `ControlSession` owns
socket generations, authentication, reconnects, host greetings and pending
settings. `MotionTranslator` owns pointer slots and ordered Android samples;
`PenMessage` and `TouchMessage` own their JSON field layouts. Connection callbacks
and touch handling share the facade monitor so input cannot overtake the opening
handshake. The control session accepts a WebSocket factory for isolated tests.
Host control transport/controller leases live in `input.rs`; `input/wire.rs`
owns the JSON schema. Dispatch uses narrow `InputSink` and `SettingsSink`
interfaces. The Linux backend owns uinput devices and follows attachment, mode
and card changes; KWin/X11 output selection lives in its mapping adapter.
`SessionSettings` applies geometry/configuration policy, including the existing
live persistent auto-resolution setting. Greetings still snapshot live encoder
settings. Adapter contract tests run without desktop services or real devices.

The shared `input-motion.json` fixture is checked against Android translation
and Rust deserialization/serialization; stylus history has separate ordering,
pressure, tilt and eraser coverage.

Every control send checks transport acceptance. A refused send cancels that
socket, clears authenticated state and local touch slots, and schedules a new
connection. Host controller teardown releases active contacts. Interrupted input
is not replayed; accepted samples keep their original order. The latest encoder
settings are replayed on reconnect. A pending mode choice survives refusal and
is cleared once queued; queue acceptance does not prove delivery or host adoption.
No second application queue or history/ACK sampling is introduced.

Scalar counters record accepted/refused sends, current and peak socket queue
bytes, and Android sample age at enqueue. They retain no message content and are
logged on explicit disconnect. Queue bytes exclude framing and OS buffering;
sample age uses Android uptime and excludes time waiting inside the socket queue.
See the [replay workload and measurement limits](benchmarks.md#android-control-replay).

Examples of individual client messages (one JSON object per WebSocket message):

```json
{"type":"touch","x":0.5,"y":0.3,"pressure":1.0,"action":0,"slot":0}
{"type":"pen","x":0.5,"y":0.3,"pressure":0.8,"tilt_x":12.0,"tilt_y":-3.0,"eraser":false,"button":false,"action":2}
{"type":"resolution","width":2960,"height":1848,"width_mm":314,"height_mm":195}
{"type":"config","bitrate":20000,"fps":60,"encoder":"h264_nvenc"}
{"type":"mode","pen_only":true}
{"type":"rendered","seq":1234,"decode_us":14200}
```

Coordinates are normalized to 0–1; wire tilt values are degrees. Touch actions
are 0 down, 1 up, 2 move; pen adds 3 hover, 4 exit/cancel, 5/6 stylus-button
down/up. Pen tip-up publishes its final axes and releases pressure/touch while
retaining tool proximity. Exit/cancel and controller teardown release the tool,
tip and button. Positional pen samples carry optional `button`, the current
primary stylus-button state; legacy clients may omit it and use actions 5/6.
Changed button state is synchronized after tool entry and before tip-down,
restoring a held modifier across Android's hover-to-contact transition.

Android [hover events](https://developer.android.com/reference/android/view/MotionEvent#ACTION_HOVER_EXIT)
refer to a view/window, not an unambiguous hardware proximity signal. UScreen
ends its virtual tool proximity on hover-exit, including view-boundary exits,
and restores it from subsequent down/hover samples. It does not infer continued
physical proximity or delay release after a real exit. A reconnect releases
previous controller state; new samples report the current button state again.
`pen-lifecycle.json` exercises stylus/eraser, held-button contact, tip-up, real
exit, cancellation, view boundaries and reentry across Android and Rust tests.

Physical dimensions, eraser and decode timing have defaults when omitted;
config messages may omit settings they do not change. The Linux
input adapter clamps tilt to ±90° and emits milliradians with an axis resolution
of 1000 units/radian. This matches [libinput's angular conversion](https://gitlab.freedesktop.org/libinput/libinput/-/blob/1.26.2/src/evdev-tablet.c#L371)
within 0.03° of the clamped wire value; Android's degree protocol is unchanged.

## Security model

Default authentication, local token storage and loopback binding restrict
access but do not protect against processes with the same-user/adb privileges.
Runtime directories are checked for ownership, permissions and a non-symlink
private final component before returning token/FIFO paths. Errors stop startup
or capture and are reported by doctor; unsafe existing directories are not
silently changed or replaced with another runtime location. USB carries the
stream over the cable; Wi-Fi setup opens the tablet's adb TCP listener and
carries the stream over that connection. See [SECURITY.md](../SECURITY.md) for
trust boundaries, update checks and uninstall behavior.

### Keyboard restoration state

KWin keyboard suppression requires a valid restore mode (0, 1 or 2). A saved
mode from an interrupted run takes precedence over the current desktop value.
New state is written and synchronized in a temporary file, published without
replacing an existing backup, and followed by a directory sync before suppression.
An empty, invalid, unreadable or nonregular `~/.local/share/uscreen/osk-restore`
file leaves the live keyboard unchanged and is retained for manual repair.
Failed D-Bus restoration retains valid state for retry. These rules also apply
when the KWin mode interface is absent: UScreen leaves the keyboard unchanged.

Capture keeps runtime-directory and FIFO paths as native `PathBuf`/`OsString`
values through creation, helper and encoder arguments, in-process reads,
diagnostics and cleanup. Non-UTF-8 bytes are preserved; lossy display formatting
is used only in messages. Token and FIFO resources use the same runtime directory.

### Capture orphan retirement

Before attaching, capture scans same-user processes and checks executable names
and native argument boundaries: `evdi_helper --capture-fifo <path>` or
`ffmpeg -i <path>` for this session's FIFO. Spaces, regular-expression characters
and neighboring FIFO names do not widen the match. Process snapshots are
revalidated after opening [Linux PID file descriptors](https://man7.org/linux/man-pages/man2/pidfd_open.2.html),
so signals remain bound to the original process. All selected processes receive
SIGTERM, share a 1.5-second grace, then receive SIGKILL if needed with a further
0.5-second exit budget. Failure to confirm retirement prevents new capture.
Orphan retirement requires `pidfd_open` (Linux 5.3+) and `pidfd_send_signal` to be
available; there is no PID-only signalling fallback. A run without matching
orphans does not need those syscalls. Doctor shares the capture-role/FIFO matcher
for reporting but never signals processes.
