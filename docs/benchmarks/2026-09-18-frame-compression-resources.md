# T419 — tablet CPU, memory and presentation research

**Compare changed-rectangle LZ4 and Zstandard against the existing hardware
H.264 path, with tablet CPU, memory traffic and battery cost as selection
criteria.** The earlier byte savings justify investigation, not a new default.
Ample host resources do not remove the tablet's power constraints. T419 remains
open for an Android renderer and a controlled comparison.

## Why decompression time is not enough

The [initial screen](2026-09-18-frame-compression.md) measured exact RGB
reconstruction on the host and isolated native decompression on the tablet.
Neither measurement includes the complete Android presentation path. The
current implementation configures MediaCodec with a Surface in
`android/app/src/main/java/com/uscreen/DecoderSession.kt`. Android recommends
this path because native video buffers avoid mapping/copying raw output into
application ByteBuffers. A software RGB path must justify its reconstruction
and upload costs against that existing advantage.
[MediaCodec data types](https://developer.android.com/reference/android/media/MediaCodec)

Keep LZ4 in the shortlist: in the preserved, warmed motion-rectangle samples,
its median-of-trial p50/p95 decompression times were **0.388/1.362 ms**, versus
**0.453/1.873 ms** for Zstd. These are sample timings, not CPU milliseconds per
displayed frame or cadence-weighted latency. The different compressed sizes
and full-refresh costs still need to be included. XOR remains a secondary
control: materializing and applying full-frame deltas may move more memory
than sparse rectangles, even when its payload is small.

The [USB 2 comparison](2026-09-18-usb2-compatibility.md) measured roughly
239–244 Mb/s through its ADB/hash boundary. The earlier synthetic H.264 motion
stream was about 7.1 Mb/s. For that workload, bandwidth is not a reason by itself
to replace hardware video decoding. Real photo/video and scrolling inputs are
still needed; a rectangle spanning scattered changes can approach a full frame.

## Live resource observation

On 2026-09-18 at 21:29 UTC, a read-only collector observed the installed
UScreen session for approximately 60 seconds on the RugKing Pad 2 Pro, Android
16, UMS9230E, eight online CPU cores. It did not generate visual content,
assert a frame rate, change settings, restart services, or deploy an APK.
This is an **uncontrolled current-session observation**, not a matched H.264
versus RGB benchmark or a replacement for the earlier controlled trials.

Thirteen samples, five seconds apart, read process CPU ticks and RSS. CPU
intervals use each process's host-monotonic observation midpoint, with
`CLK_TCK=100`; PID/start-time identity stayed stable. App CPU alone would omit
the separate native codec and display processes:

| Process | Mean CPU, one core = 100% | Before → after reported TOTAL PSS, MiB |
| --- | ---: | ---: |
| UScreen | 62.85% | 108.53 → 93.87 |
| Vendor Codec2 service | 18.65% | 7.43 → 7.43 |
| SurfaceFlinger | 17.35% | 88.78 → 88.81 |
| Hardware composer | 13.61% | 44.39 → 44.39 |
| `media.codec` | 0.00% at tick resolution | 1.96 → 1.96 |
| `media.swcodec` | 0.00% at tick resolution | 9.61 → 9.61 |
| Graphics allocator | 0.00% at tick resolution | 1.79 → 1.79 |

Selected external services total about **49.6% of one core** in this window.
They serve other system activity too: this is not all attributable to UScreen,
nor a complete device CPU total. GPU/decoder hardware work, other processes,
frequency changes and heterogeneous core efficiency are not represented by
adding these percentages. Never label 62.85% of one core as 62.85% of the
eight-core tablet.

UScreen's sampled `/proc` RSS was 201.88–224.51 MiB, with a 206.66 MiB median
and 59 threads. Endpoint `dumpsys meminfo --local` reports differ from `/proc`
RSS: they include platform graphics accounting and were collected outside
the timed window. The app's Graphics summary stayed at 13.62 MiB and its
Native Heap summary at 13.38–13.40 MiB; Java Heap summary changed from
30.37 to 15.69 MiB. No allocation/GC trace was captured, so that decrease
does not establish the cause or prove freedom from leaks.

The table preserves Android's literal TOTAL PSS labels; raw reports also
retain TOTAL RSS, SwapPss and each memory category. Do not add RSS, PSS and
graphics figures together or treat shared-service totals as app-owned RAM.
PSS apportions shared pages, while raw RSS and graphics accounting have
different boundaries. Endpoint PSS and five-second RSS samples can miss short
allocation peaks. [Android memory diagnostics](https://developer.android.com/tools/dumpsys#meminfo)

Collecting each sample took 0.391–0.541 seconds, including ADB and foreground
checks. This overhead was not subtracted; a future controlled comparison must
use identical instrumentation for every candidate and include uninstrumented
timing controls. UScreen remained foreground at all checks. Battery readings
stayed at 49%, 4,845,150 µAh and 31.1°C; thermal status was zero at both endpoints.
The coarse gauge and short interval establish **no energy result**.

## Memory budget before adding a renderer

At 1280×800, tightly packed RGB888 requires **2.930 MiB** per frame and RGBA8888
**3.906 MiB**. Three RGBA-sized allocations alone require **11.719 MiB**,
before compressed input, decompressor state, staging storage, textures and
display queues. These are computed payload sizes, not measured renderer PSS
or an assertion that three buffers suffice on this device.

One full RGBA pass at 60 updates/s touches 234.375 MiB/s of payload. A full
copy reads and writes that payload, or 468.75 MiB/s of logical accesses.
Cache behavior, padding, driver layouts and GPU transfers change physical
memory traffic; these figures are not measured DRAM bandwidth. A small steady
RAM footprint can still incur substantial repeated work.

The first renderer should use bounded reusable native/direct buffers, update
only changed texture regions, and present only complete validated updates.
Avoid per-frame Bitmap/byte-array allocation and unnecessary full-frame RGB
to RGBA conversions. Start with a clear ownership model and measure each copy;
do not add more buffering merely to absorb a slow consumer. A texture upload
can still cause staging or synchronization costs, and sparse uploads do not
guarantee proportional compositor/display power savings.

BufferQueue passes buffer handles between producer and consumer and uses
synchronization to govern reuse. CPU and GPU access requirements can constrain
buffer layouts. Native buffer sharing may remove a copy later, but it must be
validated on this tablet; mmap/DMA terminology is not evidence of an end-to-end
zero-copy path. Host memfd work in T418 is a separate local transport boundary.
[Android BufferQueue and Gralloc](https://source.android.com/docs/core/graphics/arch-bq-gralloc)

## Experiment order and acceptance

1. Extend the isolated replay harness with CPU/resource counters covering both
   app and relevant system services. Record process CPU milliseconds per second
   and per acknowledged/presented update, queue age/depth, drops and full-refresh
   costs. Record process restarts and reject mixed-lifetime samples. Keep
   callback acknowledgements distinct from physical presentation.
2. Add a bounded reconstruction/presentation prototype for the two rectangle
   codecs, keeping the same resolution, inputs, brightness, refresh and transport
   as the H.264 control. Measure decompression, reconstruction, upload and
   presentation separately and together. Reuse the current isolated route;
   avoid reattaching EVDI through unresolved T222.
3. Compare static, sparse text/pen, continuous motion, scrolling, photo/video,
   and transitions with repeated, interleaved trial order. Retain exact RGB
   checks and explicit H.264 quality measurements: lossless RGB and subsampled
   QP-18 H.264 are not equivalent fidelity. Include burst/full-refresh tails,
   dropped updates, first-visible latency and the cost of switching to video.
4. Record sampled steady/peak PSS, RSS, swap, Java/native/graphics categories,
   allocation rates and GC pauses. Add explicit high-water counters for owned
   buffers; sampled memory is not an allocation bound. Use short system traces
   to inspect scheduling, BufferQueue backlog, compositor work and supported
   GPU counters, while retaining low-overhead runs for performance comparisons.
   [Android system tracing](https://developer.android.com/studio/profile/cpu-profiler)
5. Exercise resize, full-refresh loss, stale base/generation, corrupt or oversized
   payloads, background/resume and repeated teardown. Require permanent automated
   correctness/lifecycle regressions before production enablement; reclaim or
   retire resources safely without allocating replacement sessions indefinitely.
6. Only candidates with useful latency/fidelity and bounded resource behavior
   advance to matched sustained USB battery/thermal comparisons. Keep supply,
   geometry, cadence and display settings fixed, and obtain an undisturbed
   measurement window. T388 still tracks the separate interrupted power-policy
   comparison. Neither lower CPU nor fewer bytes substitutes for a measured
   battery result.

Choose a default only after the complete comparison demonstrates an advantage
for the intended workload. Retain H.264 when a candidate increases tablet cost
without a useful measured gain. Thresholds for any future automatic switching
must come from those trials, including transition costs and hysteresis, rather
than from arbitrary CPU or memory limits. No additional tablet is required.

## Evidence and reproduction

The [evidence directory](2026-09-18-frame-compression-resources/) contains the
read-only observer, summarizer, raw process samples, endpoint memory and thermal
reports, APK SHA-256, device fingerprint and checksums. Raw observations are in
`observation.tar.gz`, preserving diagnostic whitespace. It contains no screen
capture or user input. The installed APK is identified by hash; these numbers
are not attributed to a fresh build of the current checkout.

Recompute from the repository root without a connected tablet:

```sh
mkdir -p /tmp/uscreen-t419-resource-reproduction
tar -xzf docs/benchmarks/2026-09-18-frame-compression-resources/observation.tar.gz \
  -C /tmp/uscreen-t419-resource-reproduction
python3 docs/benchmarks/2026-09-18-frame-compression-resources/summarize-resources.py.txt \
  /tmp/uscreen-t419-resource-reproduction/observation
cmp docs/benchmarks/2026-09-18-frame-compression-resources/summary.json \
  /tmp/uscreen-t419-resource-reproduction/observation/summary.json
```

To repeat the read-only observation, run `observe-resources.py.txt` with
`--serial DEVICE_SERIAL --output NEW_DIRECTORY` while UScreen is foreground.
The collector targets this tablet's service names and rejects missing processes
or a foreground change; it is evidence tooling, not a portable production
resource monitor. An incomplete observation must not be summarized as complete.
