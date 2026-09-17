# Staged session and media ownership decision (T401)

Date: 2026-09-17. Foundation: T368/T370/T281/T376/T377/T380.
Status: retain the existing default backend; adopt the contracts and ordered
implementation gates below. Proposed wire/control/backend extensions in this
record are not implemented merely by recording this decision.

## Decision and alternatives

Keep one owned tablet session, Linux EVDI helper process isolation, stock FFmpeg
CLI encoding and the existing Android wire format as the supported default.
Improve ownership and measurement within those boundaries first. Keep optional
libavcodec experimental until its complete lifecycle and target hardware pass
the same acceptance checks. No FFmpeg patch is required or permitted.

| Alternative | Benefit | Cost and decision |
|---|---|---|
| Helper → NV12 FIFO → stock FFmpeg CLI | Established recovery, encoder process isolation, broad distro deployment | Raw transfers copy bytes and require whole-frame quarantine after partial writes. Retain as fallback and comparison baseline. |
| Helper → FIFO → in-process libavcodec | Packet ownership and writable-plane experiments can avoid selected user-space copies | Codec failure affects the daemon; FIFO still copies, frame lifetime needs explicit ownership, VAAPI hardware contexts remain unsupported. Keep optional. |
| Helper → shared-memory frame ring → encoder | Can avoid transferring full raw frames through the pipe | FD handoff, slot retention, memory ordering, crash recovery and size changes become protocol obligations. T389 must measure a bounded prototype before promotion. |
| Platform capture/hardware surfaces → hardware encoder | Potentially avoids CPU conversion and copies | Requires actual compositor/driver support, device/modifier matching and synchronization. Linux PipeWire/DMA-BUF and Windows capture are separate adapters, not universal replacements. |

A single process-wide rewrite would discard working fault boundaries before
measuring its benefit. The shared session owner already removes primary/extra
pipeline duplication; the next changes should replace one adapter at a time.
The [Windows plan](../windows-port.md) remains the platform roadmap. Driver,
packaging and physical Windows validation decisions remain pending there.

## Ownership contracts

* **Session:** `session::Runtime` owns worker tasks and shutdown. Both listeners
  must bind before any worker starts. Attachment identity/epoch retires old
  control leases synchronously, even if watch notifications coalesce. Geometry
  from a retired attachment cannot authorize a new capture. Disconnecting one
  extra slot must not retire another slot's resources.
* **Raw frame:** a lease specifies format, dimensions, strides, plane ranges,
  sequence, capture timestamp and owner generation. Storage remains immutable
  and alive until its last consumer releases it. The current C exchange already
  leases pointer/size/generation/time; its validated mode supplies the NV12
  layout. A new transport must carry the full layout explicitly. No resize,
  truncation or producer reuse while an encoder retains the lease. Exhausted
  slots apply admission/backpressure before capture/encoding; never overwrite
  an in-use slot or drop the tail of a raw FIFO frame.
* **Encoded packet:** `VideoPacket` owns immutable bytes plus the exact CSD for
  that access unit, sequence and encoder-generation identity. Broadcast clones
  retain storage; encoder retirement invalidates eligibility to send, not memory
  safety. T402 must retain an AVPacket reference or move its reference into an
  owner, never borrow the encoder's reusable local packet. T407 slices must
  account for the whole retained backing allocation.
* **Android:** `SessionCoordinator` owns session transitions, `VideoTransport`
  owns connection sockets, `VideoPacketReader` lends bytes only until the next
  read, and `DecoderSession` owns codec/output callbacks. A queued/direct-buffer
  adapter needs its own lease; it cannot retain the packet reader's borrowed
  array. A blocked socket read cannot prevent codec/surface retirement.
* **Platform:** portable protocol/policy depend on input, frame-source and
  lifecycle interfaces. Linux uinput/EVDI/process details stay in adapters.
  Unsupported capabilities fail explicitly; a Windows compilation does not
  certify monitor or pointer support.

For future cross-process messages use a session nonce plus attachment, mode and
encoder epochs. Never infer identity solely from a recycled slot, PID or frame
sequence. Existing in-memory Arc identities and C generation checks remain the
current implementation; adding serialized epochs requires fixtures on both ends.
Retirement stops admission, invalidates the epoch, interrupts waits, releases
consumer leases, joins owners, then frees storage. A failed cleanup retains
storage until ownership is resolved or the containing process exits. T226's
inode replacement is still mandatory on the FIFO path.

## Separate three timing policies

| Policy | Responsibility | Change boundary |
|---|---|---|
| Capture mode | Desktop pixels, physical size, scanout refresh and source damage | Deliberate monitor changes may recreate the EVDI helper. |
| Encoder admission/pacing | Maximum encoded rate, unchanged-frame keepalive, IDR recovery | Prefer live admission/control independent of display lifetime. |
| Android presentation | App-window brightness/refresh, decoder scheduling and rendering | Foreground app scope; restore normal behavior when another app gains focus. |

Currently `EncoderSettings::helper_geometry` includes FPS. Sending normal FPS
configuration changes therefore rebuilds the helper and EDID. Do not implement
rapid automatic battery adaptation by repeatedly editing that setting. Before
such adaptation, introduce an acknowledged, generation-tagged helper pacing
control independent of capture mode, test it with a fake helper, and preserve
CLI versus in-process IDR timing. An explicit user FPS edit keeps its current
meaning until that compatibility boundary is implemented and documented.

The battery profile is explicitly opt-in. Preserve 60 FPS, app-only 50%
brightness and 60 Hz defaults. Manual preferences and foreground transitions
must have clear precedence. T388/T399 must measure hysteresis, idle-to-motion
recovery, latency and quality before selecting adaptive behavior. Decode-all /
render-latest may discard presentation, but cannot arbitrarily omit compressed
reference frames or silently reinterpret render acknowledgements.

## Capability and compatibility boundary

Add negotiation only when a new backend/codec/control actually needs it. An
optional version/capability extension to the existing authenticated greeting is
the preferred first step. Keep existing required fields and absence-means-legacy
semantics. Require an explicit client acknowledgement before sending a new
packet type, codec or frame layout; unknown fields alone are not proof of support.
Capabilities describe actual codec/profile/size/rate support, input devices,
backend transport, supported pacing controls and effective resource limits.
The selected configuration must be reported separately from advertised support.

Fallback is stock H.264/HEVC with the existing framing and validated settings.
Do not send AV1/VP9 merely because Android lists a MIME type. T400 requires
hardware inventory and matched quality/power measurements. Authentication and
attachment validation precede capability updates; an old socket cannot revise
its replacement's policy. Joint mode/FPS validity remains T332's unresolved
user policy; this record does not choose a fallback on the user's behalf.

## Resource accounting and observability

Keep the four-tablet limit. Current eight-packet broadcasts and sixteen video
connections bound counts, not retained payload memory or blocked send duration.
T391 must measure and enforce per-session byte/time limits before increasing
capacity. Charge unique backing allocations once at the owner and track their
per-consumer retention separately; otherwise shared packet clones overcount
memory while small shared slices undercount it. Raw rings have a fixed slot and
byte budget; setup/authentication/control work needs its own admission bound.
One slow client must time out or recover at CSD/IDR without delaying other slots.

Expose bounded per-session counters for capture, conversion, encoding, send,
receive, decode and presentation, plus queue bytes/age, leases, retired packets,
recovery cause, connections, tasks and FDs. Preserve monotonic clock domains;
only subtract timestamps measured in the same clock domain. Use sequence/epoch
correlation for traces, not implied host/tablet clock agreement. Histograms need
clear aggregation semantics. Hidden UI telemetry should not cause perpetual
polling. Collection is bounded and optional, and must be benchmarked itself.
Fake-tablet ACKs represent receipt with synthetic decode time, not real rendering.

## Implementation order and acceptance gates

1. **Stable owners and cancellation:** the six foundation items establish
   contracts; T328 defines command groups and accurately reports delegated work
   that may continue. Keep current fault-isolated behavior.
2. **Measurement and budgets:** T408 removes fake-client copy distortion, then
   T390/T391 establish discovery fairness and session/client resource bounds.
3. **Local hot paths:** T383 measures aggregate conversion workers, T402/T407
   optimize packet ownership, T405 replaces avoidable polling, T406 batches
   input writes at existing SYN boundaries, T404 measures timing structures.
4. **Backend experiments:** T386/T403 evaluate decoder ownership/direct input;
   T389 compares raw transfer and shared memory with unmodified FFmpeg. Promote
   only individually validated adapters with a working fallback.
5. **Policy:** T399/T400 provide skip/codec evidence; T388 applies the measured
   opt-in profile. T409 reduces status collection overhead without weakening
   destructive-action process discovery. Consolidate broader test tooling last.

Contract fixtures precede each new adapter's behavioral changes. Existing tests
already establish the foundation: T368 bind failure/session ownership; T281 stale
attachment rejection; T370 fake input sinks and cancellation; T376 fragmented
framing and retired codec callbacks; T377 composition ownership; T380 independently
linked C pools/leases/callback contexts with sanitizer coverage; and T226 real
CLI/in-process recovery after incomplete frames. T401 additionally locks down
shared packet storage/CSD lifetime versus encoder retirement.

Future transport acceptance adds delayed/reordered release, pool exhaustion,
partial handoff, consumer crash, producer restart, resize while retained and
exactly-once release. Negotiation acceptance includes old/new endpoints, absent
and unknown capabilities, unsupported selection and epoch replacement. Timing
acceptance includes idle keepalives, motion resumption, sparse IDR joins and stop
while blocked. Do not replace those tests with successful live streaming alone.

Record source/toolchain/settings and raw samples for 1/2/4 sessions. Compare CPU,
allocations, retained bytes, context switches/wakeups, delivered FPS and tail
latency under static, motion, pen, slow-client and reconnect loads. Physical
Android trials also need matched brightness/refresh/charging/temperature and
sustained net-charge measurements. Synthetic results cannot establish battery
savings or end-to-end display latency. Keep a candidate optional or unpromoted
when hardware evidence is missing, and retain the outstanding TODO requirement.
