# Performance and scalability research — 2026-09-17

Scope: Android client and Rust/C Linux host at
[`6a49e9761c541fdcb224bdd6c9216f53581a1c6c`](https://github.com/geraldo-netto/UScreen/tree/6a49e9761c541fdcb224bdd6c9216f53581a1c6c).
Read alongside the [architecture review](2026-09-17-architecture.md),
[historical benchmarks](../benchmarks.md) and [Windows integration plan](../windows-port.md).
Actionable work is tracked as T382–T392 in the
[TODO ledger](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/TODO.md).

The first priority is a reproducible current-fork baseline. The code supports
specific optimization hypotheses, but this review ran no hardware streaming,
GPU, battery or multi-tablet benchmarks. Historical upstream numbers are not
fresh measurements and do not establish a speedup from any proposal below.
The two isolated GUI probes in the architecture report establish behavior only.

## Current pipeline and likely scaling pressures

The default host path is EVDI framebuffer grab → C BGRA-to-NV12 conversion →
raw FIFO → FFmpeg child → Rust Annex-B packetization → per-session broadcast →
TCP through ADB. Android reads packets into decoder input buffers, drains
MediaCodec output to a Surface, then acknowledges rendered frame sequences
over the control WebSocket. Input events use that control connection too.

The optional in-process encoder removes the FFmpeg child; it still reads raw
FIFO frames and copies planes into libavcodec-owned buffers. It is not a
zero-copy path. `stream.rs:write_frame` already uses vectored header/payload
writes, and `Bytes` clones share encoded payload storage. Proposals should
target remaining work rather than reimplement those existing optimizations.

### Resource arithmetic, not measured bandwidth

For a 2960 × 1848 framebuffer, let N = 5,470,080 pixels:

| Resource | One tablet | Four tablets |
| --- | ---: | ---: |
| Tightly packed BGRA framebuffer: 4N | 21,880,320 bytes | 87,521,280 bytes |
| One packed NV12 frame: 1.5N | 8,205,120 bytes | 32,820,480 bytes |
| Helper framebuffer plus three NV12 buffers: 8.5N | About 44.34 MiB | About 177.37 MiB |
| Full NV12 payload at 60 delivered frames/s | About 0.492 GB/s | About 1.969 GB/s |

These are minimum payload/storage estimates for scale 1. They exclude stride
and allocation padding, dirty maps, driver buffers, FIFO/kernel copies, encoder
surfaces, encoded queues and Android memory. Raw FIFO traffic is **not** the
encoded USB bit rate. At scale 2, packed output has approximately one quarter
as many pixels, but the EVDI framebuffer grab remains full resolution.

`conv_pool_init` independently chooses online CPUs minus two, clamped to 1–8,
per helper. On a host exposing at least ten CPUs, four helpers request 28 worker
threads plus four calling threads for conversion, before encoders, writers and
Tokio. Oversubscription is therefore plausible; its actual effect depends on
load, scheduling, affinity and memory bandwidth. T383 measures it.

## T382: measurement plan

Record commit, build profile/features, compiler and dependency versions, kernel,
EVDI/compositor, GPU/driver/encoder, host CPU availability, Android model/API,
decoder name, negotiated USB speed or Wi-Fi conditions, power source and exact
configuration. Include native and encoded geometry, scale, codec/profile/depth,
FPS target, quality/bitrate policy, brightness and requested/observed refresh.
Do not publish session tokens or captured user content in traces.

Use deterministic synthetic desktop content: static text, continuous scrolling,
high-detail motion and reproducible pen strokes. Start with one default session,
then vary one factor at a time. Suggested initial runs are five minutes after
warm-up, repeated at least three times with alternating A/B order; add a
30-minute sustained run for thermal behavior. These durations are an experiment
proposal, not a guarantee of statistical sufficiency.

| Dimension | Initial comparisons |
| --- | --- |
| Session count | 1, 2 and 4 tablets; mixed resolutions; one slow/reconnecting tablet |
| Host work | 1080p and native tablet geometry; scale 1/2; 30/60 FPS, then 90 only on supported configurations |
| Encoder | Default CLI versus optional in-process on supported hardware; NVENC, VAAPI and software separately |
| Android | API 27 compatibility fixture plus available physical API 29/30/34+ devices; H.264 and supported HEVC profiles |
| Transport | USB, Wi-Fi, shared USB-controller contention, reconnect and constrained bandwidth |
| Lifecycle | Streaming, pen-only, waiting, background/foreground and Surface recreation |

Measure capture wait/grab, conversion, raw-frame wait, encode, packetization,
send queue/write, Android receive/decode queue, output callback and host ACK
receipt. Attach sequence and encoder-generation identities to observations.
Host and Android clocks are separate: compare intervals within each clock,
or establish synchronization with uncertainty before subtracting timestamps.
The existing send-to-ACK timer starts after capture/encoding and is not total
display latency. Optical input-to-display measurement is a separate experiment.

Report per-session delivered/rendered FPS, p50/p95/p99 latency, stalls and frame
drops alongside CPU time, runnable threads/context switches, peak RSS/PSS,
allocations, retained queue bytes, GPU utilization where supported, event age
and power/thermal data. Preserve raw samples and configuration with each result;
do not subtract independently calculated percentiles to infer stage duration.

Linux `perf stat`/`perf record` can identify CPU and scheduling hot spots; Android
Perfetto can correlate application traces with scheduling and rendering. Confirm
available counters and tracing overhead on each device instead of assuming all
sources exist. See the [perf tutorial](https://perfwiki.github.io/main/tutorial/)
and [Perfetto Android tracing guide](https://perfetto.dev/docs/quickstart/android-tracing).

## Rust/C improvements

| Work | Code evidence | Experiment and acceptance condition |
| --- | --- | --- |
| T383: conversion/resource budget | Each C helper creates its own conversion pool and large buffers. | Sweep 1/2/4/8 conversion slots and 1/2/4 sessions, including restricted CPU availability. Compare conversion time, context switches, memory bandwidth and per-session p99. Introduce a shared host budget only if it improves aggregate behavior without starving a tablet. |
| T384: packetizer allocation/scanning | `process_complete_nals` searches the retained buffer twice, copies NAL slices and front-drains a Vec; config is copied for packet publication. | Replay identical H.264/HEVC streams with tiny/random/large chunks. Compare incremental scanning, reusable capacity and cached immutable config. Require byte-identical access units/config/generation behavior and measured allocation or CPU improvement. |
| T385: blocking persistence | `persist_settings` and `persist_mode` call file locking/fsync synchronously inside Tokio futures. | First prove the stall using a held temporary config lock and a current-thread runtime heartbeat. Use bounded owned persistence work; retain concurrent-edit merging, CLI override and retry semantics. Measure executor responsiveness separately from disk latency. |
| T390: discovery fairness | The ADB monitor awaits device identity, app probes and session setup sequentially. | Use fake ADB devices with deterministic delays/failures, then measure unaffected-device recovery. Consider bounded per-device concurrency while serializing shared forwarding operations and preserving stable identities/cancellation. |
| T391: client/session limits | Eight-packet broadcast queues bound packet count, not encoded bytes or stalled write time; each session admits multiple clients. | Mix fast/slow/unauthenticated clients and reconnect churn across sessions. Account for shared Bytes storage correctly. Establish retained-byte/time/task/FD budgets, preserving decode resynchronization at a valid generation and IDR. |

For C conversion, first profile the existing scalar native/scaled kernels and
dirty-row optimization. Then evaluate compiler vectorization or explicitly
dispatched portable SIMD against the same pixel fixtures. Preserve arithmetic,
chroma layout, stride and dirty-row behavior; avoid distributing binaries that
require the build machine's instruction set. Larger thread counts or a new
conversion library are not automatically improvements.

Blocking work must not become an unbounded collection of tasks. Tokio documents
that a started `spawn_blocking` closure cannot be aborted and can prolong runtime
shutdown; cancellation and lock acquisition need an explicit design. A bounded
worker with coalesced state and defined completion semantics is a candidate,
not a license to discard pending user changes. See
[Tokio's blocking-task guidance](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).

### T389: raw-frame IPC and hardware capture feasibility

Compare these as staged alternatives, preserving the C process boundary until
there is evidence that changing it helps:

| Option | Potential gain | Required proof |
| --- | --- | --- |
| Framed FIFO | Explicit recovery after partial writes; modest architectural change | Sequence/size/format/generation validation, bounded allocation, EOF/restart agreement and the T226 real-FIFO regression. Framing by itself does not remove frame copies. |
| Leased shared-memory ring | Fewer raw-frame copies/syscalls | Producer/consumer ownership, no overwrite before release, backpressure, format changes, stale generation retirement and crash recovery. Benchmark against the corrected FIFO path. |
| PipeWire plus DMA-BUF/hardware frames | Potentially avoid some CPU framebuffer/conversion/upload work | Desktop source availability, encoder-compatible GPU formats/modifiers, synchronization, device matching, buffer lifetime and a working fallback when direct import fails. |

The ScreenCast portal defines a VIRTUAL source for extending with a new monitor,
but consumers must inspect `AvailableSourceTypes`; existence in the protocol
does not establish backend availability on Cinnamon, KDE, GNOME or Sway.
Prototype on isolated desktops and verify extended-display behavior, consent,
input mapping and session closure. Preserve EVDI support where needed. See the
[ScreenCast interface](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html).

DMA-BUF import requires negotiated format/modifier handling and synchronization;
mapping it as ordinary CPU memory is not generally valid or efficient. Keep a
shared-memory fallback. FFmpeg hardware upload/mapping similarly requires the
appropriate device/frame context; enabling a hardware encoder name does not
make software NV12 input copy-free. See
[PipeWire DMA-BUF sharing](https://docs.pipewire.org/devel/page_dma_buf.html) and
[FFmpeg hardware-device/filter options](https://ffmpeg.org/ffmpeg.html).

Do not promote the in-process VAAPI path before T284 is resolved, or claim a
bitrate ceiling while T259 remains. Faster delivery of incorrectly framed or
unsupported frames is not an acceptable result.

## Android improvements

### T386: codec scheduling and capability-aware hints

`feedDecoder` performs up to ten 20 ms input-buffer waits while synchronized on
the receiver. Output drains on another thread; codec release and Surface/socket
lifetimes also coordinate through the receiver. Investigate one codec owner
with asynchronous buffer callbacks and a bounded access-unit queue. Compare
wakeups, blocked time, queue age, allocations and p99 under identical streams;
preserve the current path until the candidate is demonstrably reliable.

Android provides asynchronous MediaCodec callbacks from API 21. Callback mode
changes lifecycle rules, including restarting after a flush; codec-specific
data and keyframe recovery must remain correct. Keep ownership and stale
callback rejection explicit instead of combining synchronous dequeue calls
with callback mode. See the
[MediaCodec lifecycle reference](https://developer.android.com/reference/android/media/MediaCodec).

The current format sets low latency on API 30+, requests operating rate at twice
the stream FPS and adds a Qualcomm vendor hint. Test supported decoder profiles
and combinations individually, including fallback and sustained load. Android's
standard low-latency decoding feature is capability-dependent and may increase
power use; it should be evaluated alongside latency, not assumed free. See
[Android 11 low-latency decoding](https://developer.android.com/about/versions/11/features#low-latency-decoding).

Required automated coverage includes input/output callback order, queue bounds,
CSD followed by IDR, codec change, stop during backpressure, Surface replacement,
stale generations and repeated stalls. T339 must retain failure history long
enough to reach the chosen fallback. Physical decoder/power comparisons cannot
be replaced by Robolectric success.

### T387: input/control allocation and backpressure

`TouchCapture` creates JSON messages for pen history and per-frame ACKs, and
`sendWhenConnected` ignores `WebSocket.send`'s Boolean result. Measure allocation
rate/GC, queued bytes and event age before replacing serialization. A typed
serializer from T375 can enable reuse without changing the protocol.

OkHttp 4.12 reports enqueue failure on a closed or overflowing socket and caps
queued payload at 16 MiB. `queueSize` excludes framing and OS buffers, so it is
one signal rather than complete network backlog. See the version-matched
[WebSocket contract](https://raw.githubusercontent.com/square/okhttp/parent-4.12.0/okhttp/src/main/kotlin/okhttp3/WebSocket.kt)
and [queue implementation](https://raw.githubusercontent.com/square/okhttp/parent-4.12.0/okhttp/src/main/kotlin/okhttp3/internal/ws/RealWebSocket.kt).

Use a slow/refusing fake socket to test rejected sends, pending-setting delivery
and reconnect. Define overload behavior explicitly: down/up/button/cancel and
stylus history cannot be discarded merely to reduce queue length. Dropping
history changes drawing fidelity. ACK sampling would also change the existing
latency diagnostic and requires an explicit protocol/measurement decision.

### T388: refresh, power and thermal behavior

Retain the app-only defaults of 50% brightness and 60 Hz, user-selectable
overrides and restoration of normal settings outside UScreen. Record both
requested and observed display mode; Android frame-rate requests are hints,
and the system may select a different rate or multiple. Evaluate Surface
frame-rate hints only with the current window policy and API guards, without
silently replacing a user's preference. See
[Android frame-rate guidance](https://developer.android.com/media/optimize/performance/frame-rate).

The service takes a partial wake lock and Wi-Fi lock when started, independently
of stream transport or useful work. Measure streaming, pen-only, waiting and
background cases. ADB loopback addresses cannot reveal whether the physical
transport is USB or Wi-Fi; selective locking needs reliable host transport
information and reconnect handling. Preserve the measured wireless-latency
benefit where it applies, and establish USB/idle cost separately.

Low-latency Wi-Fi locking is active under access-point, screen and foreground
conditions and can trade battery/throughput for latency. This also exposes T392:
the source comment incorrectly dates HIGH_PERF deprecation to API 29 rather
than 34 and overstates transport-independent USB behavior. Correcting the
comment does not establish a measured battery saving. See
[WifiManager's lock contract](https://developer.android.com/reference/android/net/wifi/WifiManager#WIFI_MODE_FULL_LOW_LATENCY)
and the [API 34 change record](https://developer.android.com/sdk/api_diff/34/changes/android.net.wifi.WifiManager).

Under USB power, battery counters describe net charge into/out of the battery,
not UScreen's isolated consumption. Use supported power rails and, where
available, an external USB power meter alongside battery trend, charging state,
fixed brightness/workload and temperature. Rail availability varies by device.
See [Perfetto power sources](https://perfetto.dev/docs/data-sources/battery-counters).

Use API-supported thermal status/headroom with a centralized sampler; headroom
queries more often than once every ten seconds can return NaN, and unsupported
devices must be recorded as such. Measure sustained output before proposing
adaptation. Automatic resolution/FPS/brightness changes require a separate
product decision; this research does not authorize them. See
[Android thermal guidance](https://developer.android.com/games/optimize/adpf/thermal).

## Execution order and completion criteria

1. Establish T382 traces/replay workloads and resolve correctness blockers that
   invalidate measurements: T226 framing, T247 control authentication and T339
   fallback where relevant. Use isolated environments for the T222 desktop crash.
2. Measure lower-risk candidates: packetizer allocations, blocking persistence,
   helper thread budgets, Android control queues and ADB fairness.
3. After ownership extraction, compare decoder scheduling/hints and raw-frame
   IPC/backend alternatives. Test each change separately before combining them.
4. Run the sustained power and 1/2/4-tablet matrix. Keep the current four-tablet
   limit until T330 allocation and aggregate CPU/memory/GPU/USB limits are known.

Accept a performance change only with repeatable raw results, unchanged wire
and lifecycle correctness, and no unexplained per-session latency, power or
quality regression. Report unavailable hardware cases as untested. Define
hardware-specific regression tolerances from baseline variability rather than
inventing a universal FPS/latency promise.

All behavioral fixes require a permanent regression added and shown failing
before the fix, then passing afterward. Microbenchmarks and hardware traces
support that coverage; they do not replace it. Record actual missing evidence
or decisions in the ledger and retain the item until its acceptance conditions
are met.
