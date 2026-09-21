# Reader-driven EVDI conversion — T580, 2026-09-21

**The retained EVDI helper used 35.6% less CPU at the median during optional
GPU capture.** All three pairs improved, by 30.1–36.5%. This is a reduction in
one helper's CPU use, not a video-latency, whole-system or battery measurement.
The ordinary FIFO encoder remains the default and still needs NV12 conversion.

## Behavior

The Linux FIFO writer publishes reader demand to the frame exchange. With no
reader, capture skips BGRA-to-NV12 conversion and publication. Stock libevdi
continues capture requests, pixel grabs, mode callbacks and flip acknowledgements;
the BGRA framebuffer stays current. Conversion workers remain allocated but
sleep until needed. Shared-memory raw-ring capture is unchanged.

Opening a FIFO reader invalidates cached frames and wakes capture through a
nonblocking eventfd. The next publication converts the entire current framebuffer,
including on a static desktop. Reader/mode generations reject conversions started
before a transition. Reader loss pauses conversion again after the writer detects
the failed write; existing FIFO quarantine/rotation remains in force. Discovery
still uses the existing 50 ms interval. If eventfd allocation fails, the helper
logs the fallback and keeps legacy continuous conversion.

This uses the existing Linux adapter boundary; it adds no Linux-specific policy
to shared configuration, Android or the host's portable resource interfaces.
FFmpeg and libevdi remain unmodified. Kernel readback still occurs: this is not
full-pipeline zero-copy.

## Matched native measurement

Before: C helper from `22f74cb`. After: T580 helper sources retained in the
evidence archive. Both used the same generic `cc -O3` build, stock libevdi
1.15.0, automatic 30-participant conversion pool and 1280×800/30 FPS settings.
Host: Ryzen 9 7945HX; X11 producer and GPU encoder used AMD Raphael/renderD129.
The existing optional GPU helper used its unchanged H.264 VAAPI Baseline path.

The explicitly unused card2 / `DVI-I-3-2` output displayed the existing T575
29 FPS moving rectangle/barcode scene. After two seconds of warmup, each trial
encoded 480 GPU frames, taking approximately 16.04 seconds. No FIFO reader was
open during CPU collection. Order was before/after, after/before, before/after.
No builds or CPU stress ran during collection.

| Pair | Before EVDI CPU seconds | After EVDI CPU seconds | Reduction |
| --- | ---: | ---: | ---: |
| 1 | 0.72 | 0.47 | 34.7% |
| 2, reversed order | 0.74 | 0.47 | 36.5% |
| 3 | 0.73 | 0.51 | 30.1% |
| Median | **0.73** | **0.47** | **35.6%** |

Median helper load was **4.55% versus 2.93% of one CPU core**, a reduction of
1.62 percentage points. `/proc/PID/stat` supplies aggregate process user/system
time with 10 ms tick resolution. GPU-helper CPU stayed approximately 0.167–0.168
seconds per trial; that separate encoder is unchanged. Native EVDI logs retain
approximately 29 pixel grabs/s on both sides, confirming continued readback.
These measurements exclude Xorg, kernel work charged elsewhere, USB and Android.
Three short pairs do not establish long-term energy or multi-device scaling.

After each candidate trial, the benchmark stopped only its own scene process
with SIGSTOP, waited 400 ms, then opened the FIFO. All three delivered a complete
1,536,000-byte NV12 frame without resuming the scene; a known green patch matched
the converter's expected luma. Full-frame times were **41.81, 8.96 and 38.66 ms**.
These include reader discovery, conversion and pipe transfer. They are fallback
observations, not a before/after latency comparison or Android presentation test.

The temporary output was retired. Before/after desktop layouts match exactly;
daemon, GUI, live EVDI helper and FFmpeg PIDs stayed unchanged. No Android
lifecycle, ADB, power, service restart or installed-binary changes were made.

## Permanent verification

The T580 no-reader regression failed against the original helper with
`no FIFO reader must mean no NV12 conversion jobs`, then passed after the change.
The normal `cargo test` EVDI harness retains absent/early-reader, reconnect,
paused/connected resize, fresh-pixel, stale-generation, immutable-lease,
generation-wrap and eventfd-failure cases. Bounded fuzzing exercises 512 transition
sequences across even dimensions 2–32. Address/undefined sanitizers pass; the
concurrent reconnect case also passes ThreadSanitizer. Existing partial-FIFO,
mode retirement, damage, shared-slot and shutdown regressions remain intact.

The four capture/conversion integration targets passed all 51 tests during the
coverage run. The subsequently added T580 ThreadSanitizer entry and the T580
address/undefined suite both pass. All **153 C production functions** meet the
80% executable-line gate; each new demand function has 100% line coverage.
All **5,595 inventoried functions** meet cyclomatic complexity ≤9.

Reproduce CPU/fallback checks with
[`fifo-demand.py`](../../../scripts/benchmarks/fifo-demand.py) and explicit
before/after helpers, an unused EVDI card/output, the existing `gpu-scene`
executable, GPU helper, same-device render node and test EDID. `--help` lists
arguments. It does not operate on Android. Build arguments, binary/source hashes,
raw per-trial observations, native logs, regression logs and coverage results are
retained in [evidence.tar.gz](evidence.tar.gz); see [metadata.json](metadata.json)
and [results.json](results.json).
