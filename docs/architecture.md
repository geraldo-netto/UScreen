# Architecture

This describes the Linux host and Android client in the current checkout.
Windows remains a [proposed integration](windows-port.md). Known behavioral
limits are tracked in [TODO.md](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/TODO.md); describing a path does not certify
it on every desktop or device.

The [2026-09-17 architecture review](reviews/2026-09-17-architecture.md) records
merge/split opportunities, ownership boundaries and required regression coverage.
Its [performance research](reviews/2026-09-17-performance-scalability.md) proposes
Android and Rust/C measurements and improvements; these are not benchmark results.
The [staged ownership decision](decisions/2026-09-17-session-media-ownership.md)
defines the contracts, implementation order and evidence gates for raw-frame,
packet, decoder, resource-budget and power-policy changes. The current CLI
fallback remains supported; proposed interfaces there are labelled separately
from the existing implementation.

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
3. **Encode.** The default FFmpeg child uses NVENC, VAAPI, software libx264, libvpx VP9 or libaom AV1.
   Automatic selection probes compatible CLI profiles off the capture path.
   Current peers add bounded live render-ACK comparisons, a synthetic fidelity
   guard and exact decoder configuration receipts before activation/recovery;
   explicit choices remain. Capture/encoding time is outside that live ACK metric.
   VP9/AV1 framing, profiles and peer checks are described in [video codecs](video-codecs.md).
   NVENC uses VBR/constant-quality targeting, VAAPI uses CQP, and libx264 uses
   CRF with VBV limits. VAAPI CQP is intentionally uncapped; the Linux bitrate control is disabled for explicit VAAPI choices.
   VAAPI requests async depth one only when stock encoder help advertises it.
   The explicit `h264_vaapi_baseline` choice maps to stock H.264 VAAPI with
   Constrained Baseline/CAVLC; existing selections retain their profile policy.
   See the [codec measurements](benchmarks/2026-09-18-codecs.md).
   B-frames/lookahead are disabled on the low-latency paths. An optional
   in-process libavcodec encoder avoids the child process; it does not support
   `ten_bit`. The optional build rejects VAAPI, VP9 and AV1 before capture setup and rejects
   live tablet requests to select them; both use the default FFmpeg child path.
4. **Keyframes and delivery.** The FFmpeg CLI requests a random-access keyframe each second of
   capture wall-clock time, including the five-fps idle floor. Actual recovery
   also waits for capture, encoding, packetization and transport. The software
   regression checks recovery under 1.6 seconds at idle. The optional in-process
   encoder can honor a next-frame keyframe request; its periodic GOP counts
   frames. The TCP server skips a lagging client's backlog to a retained IDR,
   or waits for another IDR if necessary.
5. **Decode.** Android receives length-prefixed encoded access units through
   adb forwarding and feeds MediaCodec, rendering to a SurfaceView. It requests
   the legacy low-latency hints (including a Qualcomm vendor key) and a 2x
   operating rate without querying advertised support. Experimental profiles
   separately gate standard hints on codec capabilities or omit them. None of
   these requests confirms effective latency or throughput. MediaCodec selection
   does not guarantee a hardware decoder; support is device-dependent.
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

Timing history belongs to a decoder epoch. Arrival, output release and render
notification records use one guarded 64-arrival history; codec replacement
retires that epoch. A validated sequence-tag cache accelerates lookups while
collisions retain the newest-first history search. Render callbacks recheck
codec ownership at acknowledgement, including callbacks paused across replacement.
Duplicate callbacks retain their ACK behavior but contribute at most one split
diagnostic per arrival. Host ACK lookup uses an offset for contiguous sequences,
including wrap, and the original search for discontinuous sequences. Reports
sort outside the tracker lock and recycle their sample storage afterward.
The [T404 replay](benchmarks/2026-09-17-timing.md) records correctness coverage,
lookup tradeoffs and allocation counts; these are not display-latency gains.

## Processes and settings

The C helper is assembled from independently linked modules. `capture.c` owns
the EVDI mode, registered BGRA framebuffer and event callbacks; callbacks receive
that capture context through EVDI's `user_data`. `conversion.c` owns a worker
pool whose jobs borrow input, output and dirty masks until conversion completes.
Native and scaled conversion retain their separate inner loops and BT.709 math.
By default, the helper sizes its pool from startup CPU affinity (minus two,
clamped to 1..128 participants including the caller). The Linux-only
`conversion_threads` setting accepts 0 for Auto or a manual 1..128 capacity,
independent of the affinity heuristic. Startup logs report policy, requested
capacity and effective participants after any worker-creation failures.
Dirty-work size limits the active
jobs; empty masks skip dispatch, small updates stay on the caller, and selected
workers divide dirty rows equally. Every buffer retains its own stale-row
history. This is a per-helper work budget, not a global CPU-time quota; the
[conversion measurements](benchmarks/2026-09-17-conversion.md) document thread,
vectorization and multi-session tradeoffs.
The Linux-only `pipe_capacity_mib` preference is published atomically in the
private runtime directory after startup/configuration save. Saves retain the
cross-process configuration lock through publication, and startup reads the
latest preference under that same lock. Concurrent GUIs and startup cannot
publish an older snapshot after a newer save. Publication failure preserves
the saved preference and reports an error; a later startup can retry it. Any
requested daemon restart happens after releasing the configuration lock. The FIFO writer
checks it on open and at most once per second between complete writes. Linux
refusals retain queued bytes and are retried; T226 inode retirement remains
independent. The helper reports `F_GETPIPE_SZ` over its existing stdout channel; the host
publishes a private status snapshot bound to the helper process and FIFO inode.
GUI status never opens the FIFO, so it cannot wake an encoder waiting for its
writer or interfere with frame/EOF delivery.
See [pipe capacity](pipe-buffer.md) for settings and host-limit instructions.

`frame_exchange.c` owns three NV12 buffers with matching dirty-row histories.
Publishing swaps pointers; the writer claims an immutable lease containing the
pointer, size, generation and capture timestamp. That lease spans pacing and
every partial write. Mode retirement invalidates the generation and waits for
the lease to end before reallocating; a stalled writer keeps the old allocations
alive until shutdown joins it. The kernel buffer is unregistered before mode
replacement frees it.

`writer.c` owns FIFO pacing and idle keepalives. `fifo_writer.c` owns the active
descriptor and quarantined inode/descriptor, retaining the same `FIFO_RESET`
contract with the Rust supervisor. Shutdown joins the writer and conversion
workers before freeing their buffers. These ownership interfaces retain mutexes,
condition variables and atomics; moving state into contexts does not replace
synchronization. `evdi_helper.c` remains the process composition root for options,
device acquisition, signals and ordered teardown. The module test links the C
files separately, while the existing injected-syscall harness retains the full
helper regressions, including sanitizer checks and FIFO recovery integration.

T405 replaces avoidable periodic checks with event or deadline waits. Capture
polls until its next work deadline (or the existing 250 ms pending-update
watchdog); EVDI events can wake it earlier. A writer with a cached frame waits
until the 200 ms keepalive deadline, while one without a cached frame waits for
publication or shutdown. Native FIFO writes try nonblocking output first and
wait on `POLLOUT` only after `EAGAIN`; the one-second continuous-stall limit and
partial-frame quarantine remain. Startup without a reader still retries every
50 ms, and capture without a mode still uses its 100 ms fallback.

The optional Rust encoder polls data readiness with a latched `eventfd` stop
signal. A writerless FIFO can report persistent `POLLHUP`, so that state waits
for inode/open notifications instead of polling the FIFO repeatedly. The watch
follows `/proc/self/fd/<reader>` to bind the owned inode even if its path changes.
If notifications are unavailable or the inode moves, EOF retries fall back to
a cancellable 5 ms wait. Codec configuration uses a retained `watch` value:
publication wakes initial-config waiters without a check/subscribe race, while
the five-second startup deadline remains. Video-server stop is also latched;
input-server cancellation follows its owning task and no longer checks an
unused running flag. See the [readiness replay](benchmarks/2026-09-17-readiness.md)
for measured scope and lifecycle coverage.

The Rust `capture` module supervises settings changes, startup cancellation,
encoder sessions and retry ordering. `capture::helper` owns the helper child,
FIFO creation, preferred-card identity and geometry announcements;
`capture::encoding` owns encoder children/tasks and the in-process cancellation
event. `capture::cli_encoder` translates policy to FFmpeg arguments and drains its
encoded output. `capture::placement` handles desktop placement, while
`capture::process` provides bounded termination and validated orphan retirement.
The `media` module owns codec, packet, generation and live-settings contracts,
so streaming, input and encoding do not depend on capture management.
`ivf` reads bounded VP9/AV1 packets and classifies random-access headers.
`framed_annex_b` reads stock FFmpeg tee/framecrc metadata followed by each exact
H.264/HEVC encoded packet from the same pipe. It checks bounded stream metadata,
length, codec, timestamp order and checksum before `annex_b` assembles the packet
using `encoder_io`'s shared NAL scanner. No following picture is needed to flush
the current one. Borrowed complete NALs copy into bounded contiguous access-unit
storage. Keyframe configuration
is inserted before large slices, avoiding a second picture copy at publication.
Immutable `MediaBytes` preserve each queued packet's configuration and full
backing-allocation identity. NAL, access-unit and combined-configuration bounds
apply before queue admission. The [CLI assembly report](benchmarks/2026-09-17-cli-assembly.md)
records the ownership limits, real H.264/HEVC decode checks and comparison with
the committed T384 baseline; its former unframed read path and
[T384's original replay](benchmarks.md#annex-b-packetizer-replay) remain as test
and benchmark comparisons. The [framed packet report](benchmarks/2026-09-18-packet-framing.md)
measures the subsequent sparse-cadence improvement. `capture::fifo` coordinates replacement of a damaged
raw-frame FIFO; it creates the replacement before unlinking the old inode,
preventing inode reuse during recovery. Raw frames still have no in-band
sequence, size or generation header; both processes must use this reset
protocol rather than assuming a close/reopen establishes a frame boundary.

For the optional in-process encoder, `encoder_storage` owns the public stock
libavcodec DR1 allocation callback and its synchronized allocation registry.
Large known buffers publish an immutable packet owner; small or unknown buffer
views copy. Publication releases unrelated packet side data and charges the
complete retained allocation, including padding, to the session's encoded-data
budget. A stable callback context outlives the codec, while delayed consumers
can outlive both. The [packet-storage report](benchmarks/2026-09-17-packet-storage.md)
describes the ownership tests, fallback and measured stage costs.

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

The ADB monitor (`monitor.rs`) consumes device results independently. Discovery
keeps the last confirmed package eligibility while a transport remains in the
inventory. Failed or malformed `pm path` results mean unknown, so they cannot
replace an attached tablet or admit an unverified new one (T413). A successful
package-path response confirms presence; an empty normal absence response
confirms removal. This follows Android's
[package-path command](https://android.googlesource.com/platform/frameworks/base/+/main/services/core/java/com/android/server/pm/PackageManagerShellCommand.java).
Failed, timed-out or malformed device listings also preserve the last confirmed
inventory (T443). A successful, correctly headed empty listing confirms that
no usable device remains; offline/unauthorized entries are not usable transports.
Discovery
owns up to four concurrent identity/package probe sequences; device mutation
jobs own up to four forwarding/launch/recovery sequences, with one mutation
owner per transport. The two reverse routes and app launch stay ordered within
that owner. A separate single job serializes this monitor's global inventory
and Wi-Fi reconnect commands; it does not serialize independent CLI invocations.
Existing eligible devices remain available while their refresh is pending.
Known physical identities keep the USB preference and stable primary selection.

Pending extra runtimes belong to the monitor, not to command futures. Disconnect
cancels that transport's work and retires its runtime independently; a retiring
slot cannot be reused until cleanup joins. Promoting an extra to primary retires
its former slot and readiness before replacing its forwarding. Daemon shutdown
aborts and joins inventory/device jobs, then joins all retiring runtimes. Command
cancellation has the delegated/privileged-work limits described in
[development](development.md#lifecycle-command-deadlines); it cannot undo
remote ADB actions already accepted. The
[discovery replay](benchmarks/2026-09-17-discovery.md) measures local fairness and
shutdown without touching EVDI or a desktop.

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

GUI status owns a cancellable poll worker. It revalidates a cached daemon
identity each cycle and falls back to an owner/name-prefiltered inventory;
destructive actions retain full discovery. Program/autostart probes expire
after ten seconds or UI action completion. Dynamic setup/session state still
refreshes every cycle; ADB device queries are skipped without live assignments.
Status changes request repaint, while pending actions retain a short repaint
timer. Dropping the window stops future samples after any in-flight probe.
The [status-polling fixtures](benchmarks/2026-09-18-status-polling.md) document
probe counts, refresh limits and lifecycle regressions.

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

Explicit launch/reconnect still delivers credentials through the shell-permission
protected `TokenActivity`, which opens the launcher. Authentication retries use
the equally protected `TokenReceiver` broadcast instead (T420); they never start
an Activity or foreground service. A started Android session observes token
changes and reconnects with the new credentials. A stopped session stays stopped
and consumes the latest stored token when the user returns. Update host and APK
together: an older APK without the receiver cannot consume this recovery path;
the explicit launch gate remains compatible. Tokens travel on ADB shell stdin,
never in the local ADB process arguments. Fake-ADB and API 27/34 lifecycle tests
cover delivery, permission declarations, active rotation and background resume;
physical replay is not established by those fixtures. The broader T382
validation campaign is closed as `wont_fix` for now.

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
| Type 0 payload | H.264/HEVC Annex B parameter sets, or the [VP9/AV1 configuration envelope](video-codecs.md#framing) |
| Type 1 payload | Four-byte unsigned big-endian sequence number, then encoded access-unit bytes |

The codec is announced on the input/control connection; there is no separate
codec-name field in the video packet header.

Each session retains at most eight broadcast packets and admits at most sixteen
video connections, including pending authentication. `video_queue` also limits
admitted encoded backing storage to 32 MiB per session: frame and CSD clones,
queued batches, initial/last-sent headers and slices share one charge until their
last owner drops. A slice retains the original allocation's full capacity.
Encoded storage cannot move into another session without new ownership/accounting.
Four sessions therefore admit at most 128 MiB of this storage; this is not a
process RSS cap. Unpublished encoder/parser buffers, allocator metadata, raw
frames and kernel socket buffers are outside that bound.

Frame payloads must fit Android's existing 8 MiB + 1 wire-length limit (including
type and four-byte sequence); CSD has the same wire limit without a sequence.
A refused frame starts an IDR recovery interval: dependent frames are refused
until an IDR is admitted. The optional encoder receives a keyframe request;
CLI FFmpeg keeps its existing periodic IDR schedule. Sequence allocation and ACK
meaning remain unchanged. Initial cached CSD is charged and validated before
it is sent, even when no frame has arrived.

A whole framed write and a whole retained batch each have a one-second deadline.
Expiration closes that viewer and releases its subscription; a partially sent
packet is never resumed on a new connection. Batches reuse bounded deque storage,
consume at most eight packets per drain, and retain the existing latest-IDR
backlog rule. Control admission remains independently capped at sixteen sockets
with its three-second upgrade/auth deadline. See the
[resource replay](benchmarks/2026-09-17-stream-resources.md) for measured scope,
policy rationale and exclusions.

### Input/control WebSocket

The first message is `{"type":"auth","token":"…"}` when authentication is
required. A representative host greeting is valid JSON:

```json
{"status":"connected","width":2960,"height":1848,"fps":60,"codec":"h264","pen_only":false,"touch":true,"pen":true}
```

The server sends `status: "mode"` for subsequent mode/settings notifications.
`video_width` and `video_height` describe the requested encoded dimensions for
[decoder capability negotiation](video-codecs.md#negotiation).
Android validates codec, encoded dimensions and FPS as one control format for
both capability queries and decoder setup. A format change retires the old
receiver generation before publishing all fields and restarting. Native panel
updates do not overwrite the negotiated size, and queued greetings from retired
control connections cannot apply it. Older hosts that omit both encoded fields
retain the native-panel hint (1920×1080 when unavailable); they receive no new
capability query. Explicitly malformed formats stop video instead of configuring
an unsupported size. VP9/AV1 also retain their framed configuration headers.

`codec` is `h264`, `hevc`, `vp9` or `av1`; FPS is omitted if no shared settings source exists.
`width` and `height` describe the requested virtual-display geometry before
stream scaling. They share the same settings snapshot as FPS, codec and encoded
dimensions, including after negotiation and reconnect. Before negotiation the
configured geometry is used; without shared settings, startup input dimensions
remain the fallback. These requested sizes do not confirm display attachment.
An invalid resolution/FPS update leaves the settings watch unchanged, so it
cannot retire the working capture session. The requesting control socket receives
`status: "settings_rejected"`, an `error` string and the current `fps`/`bitrate`.
Rejected config replies also echo the submitted fields in `requested`; Android
ignores replies superseded by newer requests and checks queued UI callbacks
against the current request and connection generation. A matching rejection
restores confirmed preferences, corrects reconnect parameters and shows the
reason in the settings sheet. Older clients may ignore this added status.

The Android client marks its control connection authenticated only after
`status: "connected"` from the current socket. Disconnects clear that state;
callbacks from replaced sockets cannot restore it. Rust serialization and
Android's pen-mode UI use the same `control-connected.json` regression fixture.

`MainActivity` is Android's composition and lifecycle entry point. Its
`SessionCoordinator` owns control/video transitions, token replacement, user
settings events and observable state for that Activity instance.
Its `ReleaseCheckOwner` owns the optional update request independently of media
work. It cancels unfinished requests on stop or when checks are disabled, and
checks request generations again when queued results reach the UI. Resuming
can retry a cancelled check; a completed check is not repeated for that Activity
unless the preference is disabled and re-enabled. `HttpReleaseChecks` uses
OkHttp's asynchronous calls with a 15-second whole-call deadline, in addition
to 10-second connect/read timeouts, so a trickling response cannot retain the
request indefinitely. Tests use injected calls and a local HTTP endpoint.
`ActivityWindowPolicy` owns brightness/refresh overrides, immersive mode and the
orientation sensor. Focus regain still rereads the saved window preferences;
backgrounding stops streaming and unregisters the sensor. Preferences affect
only this app's window, preserving other apps' system display settings.

Compose rendering (`UScreenUi.kt`) observes `StreamPresentation` and immutable
`SettingsValues`; it emits `SettingsEvent` commands instead of accessing Prefs,
VideoReceiver or TouchCapture. The settings sheet composes focused display,
orientation, mode, stream and update sections. Draft bitrate/FPS changes still
require Apply; local display controls apply immediately. Recomposition does not
create/restart a session, and queued video/control callbacks retain their
ordering and generation checks. Hidden statistics no longer start a one-second
presentation sampling loop; visible statistics still update once per second.

`StreamingPowerBinding` owns the Activity's power-state observer from start to
stop. It combines authenticated control/route state, video readiness, acknowledged
pen-only mode and the saved opt-in battery preference. Stops cancel observation
before stopping the service, so retired Activities cannot restart it. Rejected
service starts degrade without killing streaming. `StreamingService` promotes
in `onCreate` and validates promotion on each update; rejection/destroy/null
restart release all locks and cancel pending work. T395 still tracks the older
cold-start timeout whose exact failing sequence remains unknown.

Normal mode preserves its CPU and Wi-Fi locks. Battery mode relies on the visible
Activity's existing screen-on flag, omitting the extra partial CPU lock. Active
network or unknown-route sessions retain the Wi-Fi lock; confirmed USB releases
it immediately. An inactive network/unknown session releases it after five seconds;
repeated inactive updates do not extend that deadline, recovery cancels release,
and Activity stop releases immediately. No brightness, refresh, FPS, scale, quality
or decoder profile changes are implied by the battery switch.

The monitor supplies actual ADB transport to each attachment generation. The
accepted control lease snapshots that route into an optional `transport` greeting
field (`usb` or `network`), independently of physical identity. Android resets it
on connection retirement; old hosts or unknown values remain conservative. The
loopback socket address and USB charging state never determine the route. Network
ADB can use a medium other than Wi-Fi; this metadata does not identify the radio.

Android's `VideoReceiver` coordinates run generations, Surface readiness and
visible connection state. `VideoTransport` owns the socket from before blocking
connect until retirement, with a socket factory for tests; an old worker closes
only its own socket. `VideoPacketReader` owns reusable framed-input storage and
passes borrowed config/frame payloads through `VideoPacketSink`. Consumers finish
using that storage before the next read. Framing tests cover split headers,
payloads, EOF, invalid lengths/types, sequence wrap and buffer growth.

`DecoderSession` owns MediaCodec, its render/callback threads and the output
watchdog, with injectable codec/thread factories and clocks. Receiver and
decoder retain a common monitor for Surface/generation handoffs. Blocking input
and output operations borrow a `CodecLifetime` outside that monitor. Retirement
closes admission, detaches the codec and waits at most 500 ms for its cleanup
worker; native stop/release waits until every borrowed operation finishes. A
process-wide retirement registry prevents a newly created Activity/receiver
from allocating another codec while native cleanup remains unfinished. This is
a retirement barrier, not a one-active-codec limit. A permanently stuck native
call retains its storage and prevents decoder reconnection in that process.
Codec setup runs on the receiver's I/O worker. It reserves a startup under the
shared monitor and process-wide startup gate, then creates, configures and
starts the codec outside the receiver monitor. It rechecks ownership and
Surface/run validity between stages and publishes the finished codec under the
monitor only if the attempt is still current. Invalidating a pending startup
prevents publication; the worker retains its local codec until the native call
returns, then retires that unpublished codec. The startup gate prevents another
attempt across Activity recreation until this attempt finishes; unfinished
native cleanup continues to block new allocation through the retirement registry.

Codec invalidation interrupts the transport so reconnect obtains fresh
configuration and a keyframe. Feeds check run generation and codec identity;
output workers revalidate ownership before updating the watchdog. Render
callbacks revalidate ownership before acknowledgement. `FrameTiming` owns
arrival/release measurements per epoch; `ReceiverStatistics` retains separate
counters for each run.

Synchronous input/output and the legacy hint profile remain the default.
Experimental callback operation confines codec input/output to a Handler and
owns at most two detached access units, including the one being submitted.
Admission and pending input share a 200 ms deadline; expiry resets/reconnects
rather than dropping a dependent encoded frame. Closing the mailbox wakes
blocked producers. Callback mode does not call synchronous dequeue methods.
The normal-priority callback Handler is separate from the selectable synchronous
output-thread priority. The [decoder replay](benchmarks/2026-09-18-decoder-profiles.md)
records the measured tradeoffs and the limits of render-notification timing.

T403 adds an experimental synchronous `ChannelPacketReader`/`feedDirect` path.
It validates the wire prefix before borrowing a codec input slot, reads the
payload into that slot outside the receiver monitor and queues only a complete
payload. An absolute packet deadline and channel close bound transport reads;
retirement keeps borrowed native storage alive until the reader returns. This
path requires its caller to close the transport on retirement and is incompatible
with the callback mailbox. `VideoReceiver` continues using reusable heap staging:
the [socket-input replay](benchmarks/2026-09-18-decoder-input.md) found no useful
latency improvement on the measured tablet. Direct buffers remove one application
copy here; they do not establish kernel-to-decoder zero-copy.

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

`input::event_writer` serializes native `input_event` fields into a reusable,
zeroed 64-event buffer per device. Each existing `SYN_REPORT` boundary flushes
that frame immediately; proximity, button and tip frames stay separate and no
future input sample is awaited. Native sizes/offsets come from libc, keeping
timestamps and padding initialized without reading Rust struct padding. The
writer retains `write_all` short-write/interruption handling. A failed batch is
retired before I/O so the next sample cannot replay a delivered prefix; already
accepted kernel events cannot be rolled back. Overflow fails without emitting
a partial batch, and dropping a device does not flush unfinished input. Generic
writers exercise the same pen/touch methods in the normal regression suite.
See the [T406 replay](benchmarks/2026-09-17-input-batching.md).

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

T399's optional `DecodedOutputDrainer` prototype decodes all input and can release
older already-decoded outputs without presentation, with a four-output batch cap.
It is disabled by default: the [cadence and burst experiments](benchmarks/2026-09-18-frame-pacing.md)
found no ready-output backlog or latency advantage. Capture retains its 200 ms
keepalive; the tablet's sparse-input delay argues against lengthening it without
new device evidence. This does not enable arbitrary encoded-reference dropping.
