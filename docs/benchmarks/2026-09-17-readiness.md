# Host readiness and cancellation — 2026-09-17

T405 adopts immediate FIFO readiness, latched cancellation, notification-based
codec headers, exact capture/keepalive deadlines and optimistic native writes.
The selection prioritizes host latency and sustained throughput over CPU or RAM
savings. The four-session large-frame replay uses more CPU; this is an explicit
tradeoff, not a claimed CPU optimization. Android battery behavior is unmeasured
here and remains a separate objective.

The best-effort **1 MiB pipe request remains unchanged**. Bigger queues can hold
older video even when memory is plentiful. The [capacity ramp](2026-09-17-pipe-capacity.md)
and [io_uring evaluation](2026-09-17-io-uring.md) retain their separate measurements;
neither justifies a larger default or a production io_uring dependency.

## Paced transfer results

The main series contains **120 trials**: 1/2/4 sessions, two active frame sizes
and two idle states, with five alternating baseline/candidate pairs per case.
The actual pre-change reader/writer come from `48e9427`; the candidate source
snapshot is preserved alongside the raw results. Active sessions send 120
frames on a 60 FPS schedule. Backpressure can make delivery miss that schedule.

Receipt age runs from immediately before the producer starts writing to the
consumer completing the whole frame. Each trial reports the worst session's
nearest-rank p99; the table reports the median of five trial values. It is a
**raw FIFO stage measurement**, excluding EVDI, conversion, encoding, ADB and
Android. CPU is total process CPU per trial, including producers, readers and
byte validation, not a percentage or a delay per frame.

| Frame bytes | Sessions | Receipt p99 ms, before → after | CPU ms, before → after |
| ---: | ---: | ---: | ---: |
| 1,536,000 | 1 | 5.965 → 1.993 | 254.8 → 176.5 |
| 1,536,000 | 2 | 4.648 → 1.145 | 257.3 → 224.7 |
| 1,536,000 | 4 | 6.182 → 1.684 | 618.3 → 618.4 |
| 8,205,120 | 1 | 21.081 → 5.361 | 562.2 → 589.6 |
| 8,205,120 | 2 | 22.640 → 6.894 | 1,328.6 → 1,416.0 |
| 8,205,120 | 4 | 22.381 → 7.261 | 2,398.1 → 3,064.6 |

All effective capacities were 1,048,576 bytes. The complete summaries include
min/max repeats, read/write/poll counts and voluntary context switches. Shared
host load produced substantial CPU variation, especially in the first smaller
frame case (baseline 122–323 ms). Do not interpret the median CPU changes as
stable whole-application savings.

## Separate reader/writer control

An additional **80 trials** isolate the reader and writer changes at 1/4
sessions, both sizes and five alternating rounds. Every trial uses the same
byte checks and includes its exact source and executable hashes. These are a
separate series; compare variants within this series.

| Frame bytes / sessions | Variant | Receipt p99 ms | CPU ms |
| --- | --- | ---: | ---: |
| 1,536,000 / 1 | baseline | 4.336 | 126.1 |
| | both changes | 1.292 | 112.2 |
| | reader only | 1.172 | 116.5 |
| | writer only | 4.489 | 131.7 |
| 1,536,000 / 4 | baseline | 4.556 | 552.0 |
| | both changes | 1.570 | 525.1 |
| | reader only | 1.570 | 507.8 |
| | writer only | 4.594 | 536.7 |
| 8,205,120 / 1 | baseline | 20.644 | 510.1 |
| | both changes | 3.694 | 486.9 |
| | reader only | 4.734 | 488.2 |
| | writer only | 19.791 | 520.6 |
| 8,205,120 / 4 | baseline | 20.710 | 2,202.7 |
| | both changes | 6.974 | 2,853.6 |
| | reader only | 6.294 | 2,824.6 |
| | writer only | 21.354 | 2,244.4 |

The control associates the four-session large-frame CPU increase with immediate
reader readiness. It also reproduces the much shorter transfer tail. Removing
the sleep changes producer/consumer concurrency, so lower timer or syscall
counts alone do not prove lower CPU. Optimistic writing removes the preliminary
poll when a write can proceed; backpressure can instead add a failed write
before polling. Both behaviors are included in the operation counts.

A separate **60-trial** partial-frame coalescing experiment is retained in the artifacts.
It waits up to 2 ms only after partial progress, returns to an event wait when
progress stops, and keeps cancellation interruptible. This experimental policy
is **not enabled**: at four large-frame sessions its receipt p99 was 20.472 ms,
versus 6.825 ms for immediate readiness in that series (CPU 2,188.2 versus
2,761.3 ms). The selected policy prioritizes immediate readiness.

## Idle states and cancellation

Each idle trial lasts one second after the synchronization barrier. An empty
FIFO has a writer attached but no bytes; the no-writer case exercises EOF/HUP.
The table gives median total reads and total process CPU across all sessions.
Stop latency is the median of the slowest session's completion interval from
the shared cancellation request, not a p99 estimate from five observations.

| State | Sessions | Reads, before → after | CPU ms, before → after | Stop ms, before → after |
| --- | ---: | ---: | ---: | ---: |
| empty | 1 | 487 → 1 | 2.485 → 0.191 | 0.809 → 0.023 |
| empty | 2 | 972 → 2 | 6.586 → 0.389 | 0.820 → 0.030 |
| empty | 4 | 1,948 → 4 | 9.215 → 0.506 | 0.840 → 0.030 |
| no writer | 1 | 198 → 1 | 1.427 → 0.210 | 1.390 → 0.026 |
| no writer | 2 | 396 → 2 | 4.100 → 0.401 | 2.397 → 0.029 |
| no writer | 4 | 792 → 4 | 5.358 → 0.509 | 1.770 → 0.034 |

The optional reader uses `poll` plus an owned, latched `eventfd`. EOF waits on
inotify events bound to `/proc/self/fd/<reader>`, avoiding both persistent HUP
spin and watching a replacement pathname. If notifications are unavailable or
the watched inode moves, the reader retains a cancellable 5 ms EOF fallback.
No privileges, host sysctl changes or FFmpeg patches are required.

## Other waits and correctness

The permanent paused-clock probes show no periodic idle wakes in the video and
input listeners. Video stop stays latched even before the server future is
first polled. Codec-header publication wakes waiters immediately; subscribing
before inspecting the retained value prevents a check/subscribe race. The
five-second initial-header deadline and existing write budgets remain.

Native fake-clock tests verify a 16 ms capture deadline is no longer cut to
4 ms, and that an idle writer waits until its absolute 200 ms keepalive deadline
instead of each frame period. With no cached frame it waits for publication or
shutdown. Spurious wakes retain the same deadline. EVDI events can wake capture
earlier; the 250 ms pending-update watchdog remains. No-mode capture still uses
a 100 ms fallback, no-reader native startup retries at 50 ms, and the native
writer's bounded stall poll checks mode/shutdown at intervals up to 250 ms.
Mode-retirement lease polling is tracked separately in T389.

Eighteen new T405 tests remain in the normal suites. Twelve optimization or
behavior assertions were observed failing before the corresponding change;
red/green logs are retained. Coverage includes idle reads, early stop, CSD
publication/deadline, inode replacement, reconnect, partial-frame discard,
cancellation during a partial frame, unavailable/removed notification fallback,
descriptor ownership, native deadlines and redundant writable-pipe polling.
Existing T226 real stock-FFmpeg/libavcodec recovery, T027 partial EOF, native
stall/quarantine, mode retirement, output-byte and sanitizer fixtures remain.

Validation: default host 234 passed / one manual benchmark ignored; optional
encoder 213 passed / two manual benchmarks ignored; 31 native helper tests,
one module lifetime test and the conversion suite passed. Both feature sets
pass strict Clippy. The actual helper also compiles and links with stock
libevdi; this check does not run a helper or change the desktop. The replay
itself validates every frame sequence and every payload byte.

## Reproduction and limits

Host: AMD Ryzen 9 7945HX, 32 allowed logical CPUs, Linux 7.0.0-31-generic.
Build: Rust 1.90, GCC 12.2, Debian 12 container `uscreen-ci:perf`; the integration
tests use stock FFmpeg 5.1.9. Installed physical-device baseline FFmpeg 6.1.1 is
separate. Builds use distinct target directories and verify distinct executable
hashes, avoiding cross-revision Cargo cache reuse.

[`readiness.py`](../../scripts/benchmarks/readiness.py) extracts the exact reader
function and compiles the actual native writer for each revision; a thin
adapter supplies the baseline's original atomic stop API. Counted read/write/
poll calls instrument both variants. Frames contain synthetic sequence/time
headers and checked payload bytes, not actual NV12 pictures. Worker allocation
and watcher setup can overlap the initial timing barrier; result formatting
inside workers and byte validation are included in process CPU. Final JSON
formatting in the parent is excluded. No other build or benchmark ran in
parallel with the reported series; ordinary desktop load was not controlled.

```sh
python3 scripts/benchmarks/readiness.py --directory /tmp/readiness-main \
  --output /tmp/readiness-main.json
python3 scripts/benchmarks/readiness.py --control \
  --directory /tmp/readiness-control --output /tmp/readiness-control.json
python3 scripts/benchmarks/readiness.py --coalesced \
  --directory /tmp/readiness-coalesced --output /tmp/readiness-coalesced.json
python3 scripts/benchmarks/summarize-readiness.py \
  /tmp/readiness-main.json /tmp/readiness-summary.json
```

Use fresh build directories. `--build-only` followed by `--reuse` separates
build activity from timing. The build needs cached/available Rust dependencies,
a C compiler and Linux FIFO/inotify/eventfd support; no capture device or input
device is opened. Unprivileged pipe-enlargement failure preserves the effective
capacity and records it in the results.

[Raw samples, summaries, source snapshots and regression logs](2026-09-17-readiness/)
retain the evidence. These stage results do not establish physical-screen
latency, Android power savings, or 128-core/NUMA scaling; those need the planned
integrated device rebenchmark.
