# T598: optimized client and thread attribution

Research complete. No application-lock bottleneck demonstrated; no lock API or
scheduler defaults changed. Encoder worker count remains the useful T600
experiment. Camera stayed off. Workload: the attached 1280×800 tablet, existing
60 FPS stream configuration, idle desktop and the same 30 Hz moving scene.

## Build and CPU evidence

An isolated full-client build inherits release R8/resource shrinking, uses a
separate `.optimized` package and debug signing key, and is **non-debuggable**
with `profileable android:shell=true`. Main production sources are unchanged;
[source hashes](artifacts/2026-09-26-followup/t598/source-identity.json) identify
exact inputs. APK, R8 mapping, variant recipe and build log are retained privately.
This is a profiling artifact, not a replacement production installation.

Both sides were sampled simultaneously for 20 seconds per workload at 199 Hz.
No sample loss reported by simpleperf. These are process CPU measurements;
Android vendor codec services and GPU execution are outside the app process.

| Workload | Host sampled CPU seconds | Android sampled CPU seconds |
|---|---:|---:|
| Idle | 0.487 | 2.528 |
| Motion | 2.729 | 6.487 |

The earlier debuggable-client numbers are historical, not a controlled A/B test
of the compiler configuration. Do not interpret their difference as a speedup.

Exact bundled x264 debug symbols were obtained by ELF build ID
`a53558b5197171b7354371efa694135bac22c265` from Debian debuginfod and verified.
Motion leaves now resolve macroblock cache loading (9.2%), low-resolution frame
initialization (3.3%), rate control (2.9%) and transform/quantization work.
EVDI conversion is 9.9%, `_copy_to_user` 8.8%, DRM cache flushing 5.9%.
These coarse shares are 543 samples, not precise function benchmarks.

A 65,528-byte DWARF stack capture **did not fix unwinding**: 275/543 motion
samples still have no callchain. They remain explicitly visible in flamegraphs;
leaf samples are retained rather than discarded. Kernel names use the same-boot
kallsyms map with bounded address matching. Android kernel addresses remain
restricted; no global kernel security settings were changed.

Interactive flamegraphs:

- [Host motion](artifacts/2026-09-26-followup/t598/graphs/motion-steady-host.svg)
- [Android optimized motion](artifacts/2026-09-26-followup/t598/graphs/motion-steady-android.svg)
- [Android optimized off-CPU](artifacts/2026-09-26-followup/t598/graphs/android-offcpu.svg)
- [Host idle](artifacts/2026-09-26-followup/t598/graphs/idle-steady-host.svg)
- [Android idle](artifacts/2026-09-26-followup/t598/graphs/idle-steady-android.svg)

## Syscalls and scheduler: separate windows

Five-second strace windows followed CPU sampling; their overhead is excluded
from the CPU results. Completed calls only are aggregated, with boundary and
noncomplete lines counted separately. Transient threads absent from snapshots
remain explicitly unmapped. Negative futex returns include normal races/timeouts;
they are not automatically errors or contention.

| Process, motion | Futex calls | WAKE calls | Futex elapsed thread-seconds |
|---|---:|---:|---:|
| Blent | 6 | 2 | 9.235 |
| ADB | 3,244 | 2,021 | 4.958 |
| EVDI helper | 2,883 | 1,735 | 19.780 |
| FFmpeg | 57,186 | 45,048 | 131.828 |

Elapsed thread-seconds add simultaneous sleeping threads. They are neither CPU
seconds nor frame latency. FFmpeg dominates wake traffic; Blent's application
mutexes are not implicated by these totals. Read, epoll and USB ioctl waits
mostly cover the observation window, consistent with waiting for frames/events.
Changing the syscall spelling would not remove codec work or required copies.

A separate ten-second motion trace records 133,511 scheduler events, with no
PERF_RECORD_LOST records. Runnable delay starts at a wakeup or runnable switch-out
(including preemption), ending at the next switch-in. Boundary-incomplete
intervals are excluded. Per-thread data is retained alongside process aggregates.

| Process | Wakeups | Runnable p95 µs | Runnable p99 µs | Maximum µs |
|---|---:|---:|---:|---:|
| Blent | 642 | 9.91 | 11.48 | 34.36 |
| ADB | 4,499 | 9.63 | 15.84 | 266.68 |
| EVDI helper | 2,112 | 10.57 | 22.50 | 60.09 |
| FFmpeg | 14,445 | 11.94 | 16.07 | 34,145.76 |

Typical scheduling delay is small. The FFmpeg maximum is a real outlier in this
trace, not proof of a persistent mutex bottleneck or a predicted frame gain.
The installed perf recorder omitted usable tracing metadata, so `perf script`
rejected this file. The archived decoder reads PERF_RECORD_SAMPLE using recorded
event IDs/sample types and the archived **same-kernel** scheduler formats. It
checks record bounds and accounts for all 133,511 events. This decoder is
specific to this recording/layout, not a general perf replacement. `/proc`
scheduler wait counters were not used because schedstats was disabled.

Optimized full-client off-CPU sampling ran in that separate ten-second window:
208.47 accumulated thread-seconds, predominantly futex/runtime parking, Binder
ioctl, looper polling and socket reads. Concurrent Android recording and host
trace collection add instrumentation overhead. Off-CPU includes sleeping and
runnable waiting; do not add it to CPU or compare its sum to ten wall seconds.
No targeted application lock counters are justified by this evidence yet.

## Reuse and limits

[Curated data and recipes](artifacts/2026-09-26-followup/t598/) include syscall
counts per thread/operation, scheduler distributions, folded stacks, source
identity and SHA-256 checksums. Raw traces, symbols, APK and R8 mapping are in
`~/.local/share/blent/profiles/2026-09-26-followup/t598/` (private permissions).
The original Android activity was restored; runtime config was not changed.
T598 closes its profiling investigation. T600 owns actual budget comparisons;
T599 owns the EVDI readback question. Complete host unwinding and device-wide
vendor/kernel CPU attribution remain measurement limitations, not claimed wins.
