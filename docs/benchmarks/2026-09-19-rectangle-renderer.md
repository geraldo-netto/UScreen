# T419 — Android rectangle rendering and resource comparison

**Lossless RGB rectangles reduce tablet CPU use for some UI workloads, but
consume more app memory and can overwhelm USB 2 on photographic content.**
The matched presentation measurements do not establish a latency winner.
Keep the installed hardware H.264 path; these results justify an experimental
UI/video fallback design, not a production default or a battery-saving claim.

This follows the [compression screen](2026-09-18-frame-compression.md) and
[resource investigation](2026-09-18-frame-compression-resources.md). At this
stage, live transport, recovery, fallback and sustained power validation
remained unresolved. The subsequent
[presentation and battery follow-up](2026-09-19-presentation-power.md) diagnoses
reproduced presentation omissions and tests sustained battery flow; it finds
no repeatable RGB battery advantage. The observations below remain the original
short-replay evidence, rather than a pooled result from both experiments.

## Experiment and boundaries

Measurements ran on the existing Ulefone RugKing Pad 2 Pro, Android 16/API 36,
UMS9230E, eight CPU cores, Mali-G57, at 1280×800, 50% window brightness and
60 Hz. The isolated `com.uscreen.rectbench` APK used a shell-only Activity;
touch, focus loss, pause or Surface loss cancels its replay. UScreen returned
to the foreground between trials. Host capture, display attachment, production
preferences and the installed UScreen APK were not changed.

The main matrix contains **45 completed trials**: five scenes × three paths ×
three rounds, 20 measured seconds after four seconds of warmup. The middle
round reverses codec order. Each trial starts a fresh test process. Separate
cohorts check presentation, idle cadence, mmap and repeated Activity retirement.
They must not be pooled as if they used the same collection conditions.

The H.264 control uses stock FFmpeg/VAAPI, Constrained Baseline/CAVLC, QP 18,
limited-range BT.709 NV12, the current exported encoder policy, and
`c2.unisoc.avc.decoder` with operating rate 120 and no standard low-latency
hint. The stream advertises 60 FPS, including sparse replay. RGB uses LZ4
1.9.4 or Zstandard 1.5.5 level 1, a reusable native decode context, a direct
RGB888 buffer and a persistent GLES3 texture. A changed bounding rectangle is
uploaded with `glTexSubImage2D`, followed by a complete texture draw and an EGL
swap. Empty updates do not draw; every replay second includes a full refresh.

The default fixture reader reuses a direct buffer sized to its largest validated
compressed packet. The optional mmap control maps the complete local fixture.
The prototype has bounded geometry, packet length and decompressed output, but
its trusted fixture format is **not a production network protocol**.

Both paths replay precomputed files from tablet storage. Results exclude live
capture, rectangle detection, compression, USB delivery and optical display
latency. The host remained streaming to the backgrounded production app;
that app's sampled CPU was zero in the main trials. Instrumentation is present
in both paths, including the same fixed decoder-metrics backing allocation.
The existing H.264 replay preloads its compressed clip; RGB uses bounded local
reads, or a full mapping in the explicit control. This is a comparison of
these local replay implementations, not equal storage or production allocation
contracts. No uninstrumented timing control was run.

## Tablet CPU and memory

CPU is the median of three per-trial process CPU-time/elapsed-time ratios.
**100% means one CPU core**, not the whole eight-core tablet. PSS is the ordinary
median of six `dumpsys meminfo --local` snapshots per scene/path. These are
sampled process costs; they are not hardware power measurements.

| Scene | H.264 CPU | LZ4 CPU | Zstd CPU | H.264 PSS, MiB | LZ4 PSS, MiB | Zstd PSS, MiB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Static text, 5 inputs/s | 8.58% | 2.45% | 2.60% | 38.14 | 57.19 | 57.08 |
| Pen, 60 inputs/s | 41.94% | 26.99% | 27.06% | 38.79 | 60.48 | 60.58 |
| Synthetic motion, 60 inputs/s | 42.25% | 36.17% | 36.41% | 40.29 | 61.64 | 61.63 |
| Scrolling text, 60 inputs/s | 41.80% | 41.06% | 42.02% | 39.59 | 61.29 | 60.30 |
| Moving photo composite, 60 inputs/s | 41.81% | 41.21% | 42.88% | 38.77 | 61.34 | 61.49 |

Pen app CPU fell about **35.5–35.7%**, and synthetic motion about **13.8–14.4%**,
relative to this H.264 replay. Scrolling and photo app CPU are approximately
tied; do not call small differences a stable win. Across the three runs, pen
H.264 was 41.51–42.50%, LZ4 26.93–27.13%, and Zstd 26.82–27.51%.

Separate Codec2, SurfaceFlinger and hardware-composer observations matter:
hardware decoding is not charged entirely to the app process. Their combined
median sampled CPU was:

| Scene | H.264 external services | LZ4 external services | Zstd external services |
| --- | ---: | ---: | ---: |
| Static text | 6.62% | 0.94% | 1.01% |
| Pen | 44.96% | 30.50% | 30.48% |
| Motion | 44.92% | 31.58% | 30.81% |
| Scroll | 44.39% | 27.32% | 26.50% |
| Photo | 44.90% | 26.81% | 28.30% |

These shared services also serve other activity. Their intervals trim initial
and final samples and do not exactly equal the app's timed interval. They are
neither exclusively attributable app cost nor whole-device CPU totals. The
GPU, video engine, memory bandwidth, CPU frequency/core placement and display
power remain outside this accounting.

RGB adds roughly **19–23 MiB median app PSS**. In motion, reported median
Native Heap was 7.63/15.34/15.33 MiB and Graphics 2.11/15.65/15.65 MiB for
H.264/LZ4/Zstd. Java Heap was 12.41/12.56/12.55 MiB. Main-matrix sampled maxima
were 41.43/63.69/63.84 MiB PSS and 137.26/161.20/161.35 MiB reported RSS.
All paths reached approximately 0.46 MiB reported swap PSS. These snapshots
are not proven peak-allocation bounds, and platform graphics accounting must
not be interpreted as a complete count of owned buffers.

During each 20-second 60-input/s trial, ART recorded one GC for H.264 and none
for rectangles. Instrumented allocation rates were about 0.164–0.168 MiB/s
for H.264 and 0.087–0.090 MiB/s for rectangles. Traces, callbacks and runtime
statistics contribute to those allocations. Large image buffers are reused;
this does not make the renderer allocation-free or zero-copy.

## Idle cadence control

The static RGB trace receives five inputs/s but draws only its one full
refresh/s. The five-input/s H.264 comparison therefore mixes codec differences
with avoided updates. Three additional H.264 trials replayed only the original
independent IDR pictures at one update/s, preserving the nominal 60 FPS codec
configuration and the same measurement/warmup durations.

H.264 app CPU fell from **8.58% to 6.21% of one core**. RGB remained around
2.45–2.60%, so cadence explains part, but not all, of this local static result.
The existing hardware path can benefit from fewer unnecessary updates without
introducing RGB transport. At five inputs/s, process CPU per recorded update
was 17.21 ms for H.264 callbacks versus 24.55/26.05 ms per RGB presentation;
the different update counts explain why the latter can still consume less CPU
per second. Neither callback count nor requested cadence is optical FPS.

## Presentation, not merely decoder callbacks

The nine-trial motion cohort uses 12 measured seconds, four warmup seconds,
and a separately polled SurfaceFlinger layer history. Only admissions between
one second after timed start and two seconds before timed end are joined, to
avoid creation/retirement truncation. The replay assigns sequence IDs as
MediaCodec PTS; this tablet exposes `sequence × 1000` in the first history
column. The second column is the actual presentation timestamp. RGB's native
EGL presentation timestamps match that same column.
[AOSP FrameTracker](https://android.googlesource.com/platform/frameworks/native/+/refs/heads/main/services/surfaceflinger/FrameTracker.cpp)

The native measurement uses supported `EGL_DISPLAY_PRESENT_TIME_ANDROID`,
which describes the start of display scanout. Pending, invalid and unsupported
timestamps stay missing. Compositor/fence timestamps are not an optical sensor
measurement. Callback arrival and `eglSwapBuffers` return are different events
and must not be substituted for presentation.
[EGL timestamp specification](https://registry.khronos.org/EGL/extensions/ANDROID/EGL_ANDROID_get_frame_timestamps.txt)

| Path | Three per-trial p50s, ms | Three per-trial p95s, ms | Joined / eligible presentations |
| --- | --- | --- | ---: |
| Hardware H.264 | 38.78, 32.05, 29.40 | 41.71, 34.80, 32.11 | 1586 / 1629 |
| LZ4 rectangles | 33.72, 31.02, 38.02 | 36.43, 33.73, 40.87 | 1629 / 1629 |
| Zstd rectangles | 30.49, 31.51, 33.85 | 43.79, 34.35, 36.63 | 1631 / 1631 |

H.264 lacks matches for 20 and 23 eligible frames in two trials, despite all
720 decoder callbacks arriving in each run. The evidence does not distinguish
compositor drops from history/collection limitations; unmatched frames are
excluded from latency percentiles, not silently counted as presented. Their
absence can bias the H.264 percentiles. Investigating this remains part of
T419. All main-matrix decoder callbacks and native RGB presentation records
were present, but those are not equivalent guarantees of displayed content.

The results overlap across runs. They do **not** support replacing H.264 for
latency, and they correct the misleading comparison between an approximately
11 ms decoder callback and an approximately 30 ms RGB presentation timestamp.

## Fidelity and transport demand

Host reconstruction checks compare all original RGB bytes. In the measured
APK's separate bounded-reader verification cohort, **980 full GPU texture
readbacks** matched their expected 64-bit FNV fingerprints across all five
scenes and both compression paths. Readback is excluded from performance
trials. A fingerprint is a practical research check, not cryptographic packet
integrity; the texture test does not verify the complete optical color pipeline.

The H.264 controls are lossy 4:2:0. Full decoded RGB comparisons, including
chroma subsampling and BT.709 conversion, measured 35.28/35.16/31.41/35.55/35.59
dB PSNR for text/pen/motion/scroll/photo. Consequently these are comparisons
against the current practical video policy, not equal-fidelity codec tests.

| Scene | H.264, Mb/s | LZ4 rectangles, Mb/s | Zstd rectangles, Mb/s |
| --- | ---: | ---: | ---: |
| Static text | 0.688 | 0.897 | 0.598 |
| Pen | 0.748 | 0.989 | 0.664 |
| Synthetic motion | 6.828 | 4.076 | 1.438 |
| Scrolling text | 5.537 | 53.873 | 36.349 |
| Moving photo composite | 3.416 | 576.539 | 561.799 |

Rates use replay duration, include rectangle record headers/full refreshes,
and exclude transport framing. H.264 rates count encoded stream bytes. This
is projected delivery demand from local fixtures, not measured streaming
throughput. The photo composite exceeds the [measured USB 2 ADB/hash rate of
239–244 Mb/s](2026-09-18-usb2-compatibility.md) by more than twofold. Large
bounding rectangles also make scrolling expensive. A working transport would
need bounded queues and video fallback before such workloads accumulate delay.

The photographic control moves an 800×800 image over a desktop. Its source is
NASA Earth Observatory's [Blue Marble 2002](https://science.nasa.gov/resource/blue-marble-2002/).
It broadens the earlier synthetic corpus but does not replace natural-video,
resize or mixed-workload tests.

## mmap and repeated retirement

Four descriptive mmap trials used motion/photo, both compressors, 12 measured
seconds and four warmup seconds. These are one trial per case, not a repeated
significance test. Motion app CPU was 36.01/36.82% for LZ4/Zstd, with PSS
62.24/61.66 MiB, approximately the same as bounded reads. Photo CPU was
40.48/42.05%, but PSS rose to **197.64/194.24 MiB**, compared with approximately
61 MiB for bounded reads. The photo mappings themselves span 137.46/133.94
MiB. Avoid mapping an entire large replay file by default: this test shows no
compelling CPU gain to offset the much larger resident working set. It says
nothing against a separately bounded shared-memory ring or other mmap uses.

Twelve additional two-second pen/motion rectangle sessions alternated codecs
without force-stopping the test process. PID and process-start identity stayed
constant. Post-retirement app PSS ranged from **41.30 to 41.79 MiB**, ending at
41.30 MiB versus 41.71 MiB initially; reported RSS ranged from 129.77 to
130.32 MiB. No accumulating process-memory growth appeared in this short
check. It is not proof against long-running leaks, stuck native calls, resize
or background/resume failures. Retired snapshots must not be pooled with
active rendering snapshots, and these no-warmup sessions are not performance
comparators.

Main-matrix battery temperatures were 31.1–31.5 °C; endpoint HAL SoC/GPU
temperatures were 35–40/34–38 °C. Overall thermal status was zero, while some
individual sensors reported status 1. Do not interpret the overall status as
proof that clocks or hardware power stayed identical. Short USB-powered runs
and coarse battery counters provide no controlled battery-life comparison.

## Reproduction and remaining work

The [harness guide](../../scripts/benchmarks/android-rect/README.md) documents
the pinned native libraries, build, fixture generation and guarded runs.
[Evidence](2026-09-19-rectangle-renderer/) preserves raw trials, source snapshots,
APK provenance, fixture hashes, image/font identities, quality results and
test logs. Large RGB/encoded fixtures, APK/NDK outputs and third-party build
trees are omitted; their generators and recorded digests remain available.
The summary script explicitly reports missing observations and rejects
verification-only/incomplete trials as timing evidence.

Validation: five JVM fixture-admission tests, 77 Python benchmark tests and
the project complexity check passed (3,907 functions, none above nine).
The pinned-source validator also has a permanent regression that failed when
an extra extracted C file was admitted, then passed after requiring the exact
upstream file set. Both measured native source trees passed the stricter
content/file-set check. The raw evidence retains that red/green result.

Next, retain the existing hardware path while prototyping negotiated RGB
format/base/generation identity, packet integrity, decompression limits,
atomic updates, queue bounds, reconnect/full refresh and automatic video
fallback. Exercise static-to-motion transitions, natural video, resize,
background/resume and longer memory lifetimes. Match presentation coverage,
then measure the actual capture/compression/USB/render path and sustained
tablet power under a controlled workload. Endpoint battery/thermal snapshots
in short trials cannot establish energy per frame or battery life.

Host-only compression remains a separate comparison against T418's shared
memory/stock-libavcodec design. Mapping a compressed replay file does not
remove decompression, RGB writes or texture upload. Full-frame RGB888 writes
alone are 184.32 MB/s at 1280×800×60, before subsequent reads/copies; smaller
damage reduces this workload. No DMA-buffer import or zero-copy GPU claim was
tested here, and no FFmpeg patch is involved.
