# Copy, mapping, loop and power review — 2026-09-17

Reviewed source: [`1e7b045`](https://github.com/geraldo-netto/UScreen/tree/1e7b045).
The repository inventory covered 215 owned files across Rust/C, Android, GUI,
common code, scripts, packaging, tests and documentation. Generated code,
dependencies, caches and build outputs were excluded, using the exclusions in
`scripts/complexity/check.py`; binary assets were inventoried, not treated as
source. Runtime copy, allocation, polling and iteration sites were inspected
alongside their ownership and regression coverage. This is a source review and
primary-source feasibility study, not a new device benchmark or a proof that
every proposed change will help.

All actionable findings are in [TODO.md](../../TODO.md): new T399–T409 and
extensions to T382/T383/T386/T388/T389/T391. The earlier
[performance research](2026-09-17-performance-scalability.md) describes its own
older source snapshot; this review accounts for the completed FIFO recovery,
capture ownership and incremental NAL work. The
[packetizer benchmark](../benchmarks.md) remains the measured baseline for that
specific component. None of these proposals requires patching FFmpeg.

## Existing work to preserve

The C helper already exchanges three NV12 buffers by swapping pointers, retains
per-buffer dirty-row history, and suppresses repeated unchanged frames between
200 ms keepalives. It can replace unpublished frames with newer ones. Rust
already shares encoded payloads and codec configuration with `Bytes`, writes
frame headers/payloads with vectored I/O, and resynchronizes slow viewers at a
keyframe. T384 removed repeated whole-buffer NAL scans and temporary NAL copies;
it did not remove input or access-unit assembly copies. Android reuses its
compressed packet array and renders MediaCodec output to a Surface.

These mechanisms must be included in the baseline. Reintroducing them under a
new name is not a performance improvement. The T226 real-FIFO regressions must
continue to prove partial-write quarantine and fresh-inode/encoder recovery.

## Copies and mapped storage

| Boundary | Current source evidence | Investigation |
| --- | --- | --- |
| EVDI capture and conversion | `evdi_helper.c:allocate_framebuffer,grab_now,publish_frame` uses a CPU framebuffer, conversion kernels and three packed output buffers. | T389: record actual page backing, faults and copy/conversion costs before comparing aligned allocation, anonymous mappings or a new hardware capture backend. |
| Raw helper → encoder | `write_fifo_bytes` sends the whole packed frame; optional `encoder.rs:run,encode` reads a Vec and copies into writable AVFrame planes. | T389: compare a contiguous-stride fast path, direct plane filling and leased shared mappings. |
| Encoded in-process output | `encoder_storage.rs` retains known large DR1 allocations through an immutable packet owner; small/unknown buffer views copy. | T402 implemented: [ownership, whole-backing accounting and measured stage costs](../benchmarks/2026-09-17-packet-storage.md). |
| CLI NAL assembly | `annex_b.rs:read_from` fills owned spare capacity; consumed offsets govern compaction and complete NALs copy into bounded contiguous access units. | T407 implemented: [bounds, decode validation and comparison with committed T384](../benchmarks/2026-09-17-cli-assembly.md). |
| Android compressed input | `receivePackets` fills a ByteArray; `feedDecoder` copies its payload into a codec input ByteBuffer. | T403: compare direct reads into owned input slots and a separately gated LinearBlock experiment. |

For T389, a memfd can provide RAM-backed file storage mapped into cooperating
processes. It still needs a size, an FD handoff and an ownership protocol.
Describe slots with format, strides, length, sequence and generation; publish
only complete frames and release a slot only after the encoder stops using it.
Guard against shrink/resize, process death and abandoned leases. Mapping the
existing FIFO does not provide this protocol. Keep the portable FIFO/CLI route.
See [memfd_create](https://man7.org/linux/man-pages/man2/memfd_create.2.html).

Anonymous mmap changes allocation, not ownership across independently launched
processes or the need for conversion. Current `MADV_HUGEPAGE` advice and
prefaulting do not establish actual huge-page use. Compare observed backing and
tail latency without requiring privileged system tuning; retain ordinary-page
fallback. See [Linux THP documentation](https://docs.kernel.org/admin-guide/mm/transhuge.html).

Direct plane filling must retain writable, correctly aligned/strided frames
while libavcodec may still reference earlier inputs. Removing
`av_frame_make_writable` without replacing its ownership guarantee is unsafe.
Stock [AVFrame APIs](https://ffmpeg.org/doxygen/5.1/frame_8h_source.html) specify
that shared frames may need new buffers and a copy before mutation. Shared CPU
buffers do not by themselves eliminate GPU uploads. PipeWire/DMA-BUF remains a
separate T389 backend experiment with format/modifier, synchronization and
extended-display availability requirements from the earlier research.

T402 now records whole allocations made by the public DR1 encode-buffer
callback and retains only known large buffers. Unknown storage and small
reference views copy into independent storage; an arbitrary AVBufferRef size
cannot prove the size of its underlying allocation. The owner releases unrelated
packet side data immediately, then keeps the immutable data buffer alive through
[`Bytes::from_owner`](https://docs.rs/bytes/1.11.1/bytes/struct.Bytes.html#method.from_owner).
T391 charges the complete known allocation, including padding, until the last
consumer releases it. The [packet-storage replay](../benchmarks/2026-09-17-packet-storage.md)
records delayed-consumer/decode tests and allocation/fill/publication costs,
including the stock allocator pool's reuse advantage. These isolated stage
measurements do not establish an overall application or battery gain.

For T403, a socket-channel prototype should validate framing before reserving a
codec slot, and it must release/retire that slot on EOF, cancellation and codec
replacement. Blocking network reads cannot hold the codec lifecycle monitor.
The current heap staging buffer decouples these lifetimes, so removing it has a
cost beyond replacing `put`. Consult the
[MediaCodec ownership/lifecycle contract](https://developer.android.com/reference/android/media/MediaCodec)
and [SocketChannel API](https://developer.android.com/reference/java/nio/channels/SocketChannel).
On API 30+, [`LinearBlock.isCodecCopyFreeCompatible`](https://developer.android.com/reference/android/media/MediaCodec.LinearBlock#isCodecCopyFreeCompatible(java.lang.String[]))
can test compatibility with specific codec names; allocation can still fail.
Keep the API 27 path and measure on physical decoders. Neither direct ByteBuffer
use nor LinearBlock promises a copy-free network-to-display pipeline.

T391's transport evaluation should retain ordinary vectored writes as the
baseline: Linux documents that `MSG_ZEROCOPY` on local TCP sockets incurs a
deferred copy. UScreen's host connection terminates at local ADB, so this is not
a demonstrated improvement for the current route. Any future transport proposal
must account for completion-driven buffer lifetime and downstream ADB copies.
See [Linux MSG_ZEROCOPY, including loopback limitations](https://docs.kernel.org/networking/msg_zerocopy.html).

## Repeated work and loops

| Item | Evidence | Comparison and correctness gate |
| --- | --- | --- |
| T383 | The reviewed baseline revisited chroma rows for overlapping rectangles and woke every worker. Its portable C native loop already received compiler vectorization. | [Implemented and measured](../benchmarks/2026-09-17-conversion.md): byte-range masks, work-sensitive dispatch up to 128 participants and specialized scale kernels; exact rounding and all three dirty histories are retained. T382 is closed as wont_fix for now; maintainer multi-tablet testing is outside the current scope, and larger-system tuning and measurements are optional user-run work. |
| T391 | `ClientPlayback::drain_batch` starts a new Vec for every batch and drains its prefix. | Reusable bounded storage or in-place selection; preserve every generation/config/keyframe decision and measure backing memory retention. |
| T404 | The reviewed Android timing history and Rust ACK deque used linear searches; reports discarded vector capacity. | Implemented guarded decoder epochs, validated lookup cache with collision fallback, contiguous host lookup and report-storage reuse. Permanent race/epoch/order tests and the [measured tradeoffs](../benchmarks/2026-09-17-timing.md) document the result; no end-to-end latency or power gain is claimed. |
| T405 | The reviewed source used 2/5 ms FIFO sleeps, 100 ms shutdown/config checks, a 4 ms capture wait cap and per-frame idle writer waits. | Implemented readiness/cancellation notifications, retained codec headers, optimistic native writes and exact work/keepalive deadlines. The [readiness report](../benchmarks/2026-09-17-readiness.md) records byte-checked 1/2/4-session replays, fallback limits and permanent stop/reopen/quarantine coverage; no pipe-default or io_uring backend change. |
| T406 | The reviewed emitter wrote each native event separately. | Implemented bounded batching within each existing SYN_REPORT boundary, with no sample delay and byte-equivalent lifecycle frames. The [counting replay](../benchmarks/2026-09-17-input-batching.md) reduces 256 complete writes to 38, preserving partial/error and teardown coverage. |
| T408 | Fake-tablet fragmented reads concatenate immutable bytes; each video body is materialized despite only type/sequence being inspected. | Reusable bounded receive storage/draining; preserve upgrade leftovers, EOF, framing and ACK sequence behavior. |
| T409 | GUI status repeats process, ADB, capability and setup probes every polling cycle. | Cheap prefilters, validated status fast paths and notifications with explicit invalidation; keep authoritative full discovery for stop/cleanup operations. |

The [Linux uinput implementation](https://github.com/torvalds/linux/blob/master/drivers/input/misc/uinput.c)
accepts multiple complete input events in a write. That supports investigating
T406, but does not authorize combining or removing the deliberate SYN_REPORT
frames used for proximity, position, buttons and tip transitions. A counting
writer and the existing event fixtures can establish syscall reduction and
byte/frame equivalence without injecting into the desktop.

Cold configuration/EDID parsing, small fixed encoder registries, icon conversion
and one-shot packaging copies showed no source-based reason to prefer mmap or
new caching infrastructure. Tests and tools were reviewed as supporting paths,
not treated as production bottlenecks. T408 is different because the fake client
is intended to participate in sustained multi-session experiments.

## Frame skipping, codecs and architecture

T399 asks whether *additional* skipping helps. Separate admission before
capture/conversion/encoding, encoded-stream resynchronization, and skipping
presentation after decoding. They save different work. Dropping arbitrary
reference pictures before decode can break subsequent pictures; skipping only
presentation does not avoid their decode cost. Measure motion quality and
input-to-display latency alongside bandwidth and CPU. Longer idle intervals
must preserve joins, fresh CSD/keyframes, watchdogs and the Android 10-second
socket deadline; control heartbeat and video delivery need explicit semantics.
Retain the latest visible image and test an immediate return to motion.

T400 compares H.264/HEVC with AV1/VP9 using actual available encoder/decoder
pairs; intra-only alternatives are worth pursuing only if evidence supports a
latency/recovery advantage. Android's
[supported-format table](https://developer.android.com/media/platform/supported-formats)
does not establish a particular device's hardware acceleration or sustained
performance. Query actual codecs, profiles, dimensions/rates and acceleration;
[performance-point data can be unavailable](https://developer.android.com/media/optimize/performance/codec).
Benchmark text and pen readability at comparable quality, including power and
thermal behavior. `Codec`, the registry, HEVC-only capability report, greeting
and Annex-B parser all need explicit codec-specific evolution and fallback.
New codecs are not aliases for the current NAL parser.

T401 ties these experiments to reusable ownership boundaries, rather than a
second architecture rewrite beside T368/T370/T376/T377/T380. A decision record
should distinguish capture timing, encoder admission and presentation policy;
raw-frame leases, encoded payloads and session capabilities need separate
interfaces. In particular, FPS currently participates in helper geometry and
recreates the helper/display when changed. Rapid power adaptation must not
blindly reuse that disruptive path. Keep stock CLI/process isolation available,
and align portable policy/interfaces with the existing Windows plan.

## Android battery investigation and acceptance

T388 explicitly covers the reported drain and a possible opt-in power-saving
profile. Begin with controlled UScreen-off, static, scrolling/video, pen-only,
waiting, background and reconnect runs on USB and Wi-Fi. Record brightness,
observed refresh, charging source, decoder identity, temperature and sustained
workload. USB battery trend is net charging minus device use; it cannot alone
identify UScreen's consumption. Use available
[Perfetto battery/power sources](https://perfetto.dev/docs/data-sources/battery-counters)
and external input-power measurements where possible.

Candidate mechanisms are state/transport-aware locks, elimination of hidden
statistics polling, T386 decoder scheduling/hints, T399 idle/admission pacing
and a measured T400 codec choice. `StreamingService` currently acquires locks
on service start; `UScreenMain` reads stats each second even when hidden; the
decoder uses a 10 ms output wait, maximum thread priority and performance hints.
Their contribution to this tablet's drain has not been measured. Android
specifically notes that [low-latency decoding can consume additional power](https://developer.android.com/about/versions/11/features#low-latency-decoding).

Specify an opt-in profile, hysteresis, recovery and user overrides before
automatic quality/FPS/scale changes. Preserve app-only 50% brightness/60 Hz
defaults and normal settings when using another app. Success requires a
repeatable sustained power improvement with explicit quality and latency
tradeoffs, correct lock/lifecycle behavior and no reconnect oscillation.

Start with existing single-tablet measurements and T408 harness reliability, then compare the
local copy/loop candidates independently. T376/T380/T386 ownership work supports
larger buffer/backend experiments. All behavioral fixes still require a
permanent regression demonstrated failing before the fix and passing afterward;
performance experiments need reproducible raw A/B results in addition. Missing
device runs remain untested, and completed synthetic NAL measurements do not
establish battery savings.
