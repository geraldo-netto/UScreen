# C conversion and worker budgets — 2026-09-17

T383 retains portable compiler vectorization and specializes the scale-2/3/4
box filters. Dirty ranges use byte-range OR/memset operations across all three
buffer histories. Empty damage skips dispatch; sparse damage runs on the caller.
Larger jobs divide dirty rows equally and signal only selected workers.

The pool supports **128 conversion participants, including the caller** (up to
127 background pthreads). At startup the helper requests allowed CPUs minus
two, clamped to 1..128, rather than using online CPU count alone. Dynamically
sized affinity masks also handle sparse CPU IDs beyond CPU_SETSIZE. An
unavailable affinity API falls back to online count; allocation/size failure
falls back to one participant. Worker creation failure retains only successfully
created workers, as before.

This capacity is not a per-frame command to wake every thread. Normal capture
supplies a dirty history: the dispatcher targets 262,144 source pixels per job,
rounds up and caps by dirty rows and available participants. Full 1080p therefore
uses at most eight jobs; full 4K can use 32. Clustered updates are divided by
**dirty** row count, not by the full surface height. NULL masks retain the
module's explicit full-pool path for fixed-width kernel measurements; production
capture supplies dirty masks. A pool has one submitting owner and joins all
jobs before returning or releasing borrowed pixels/masks.

## Measured conversion

Host: AMD Ryzen 9 7945HX, 16 physical / 32 logical CPUs, Linux
7.0.0-31-generic, GCC 13.3.0. The main replay contains 384 cases × three paired
trials = 2,304 runs: 1/2/4 independent sessions, 1/2/4/8 participants, 32 versus
four allowed CPUs, scales 1..4, and empty/sparse/overlapping/full damage.

This table uses full 1920×1080 source frames, eight-participant pools and all
32 logical CPUs. Values are microseconds: for each trial take each session's
conversion p50, average those session values, then take the median of the three
trials. These are not pooled percentiles or capture-to-display latency.

| Sessions | Scale | Baseline µs | Candidate µs |
| --- | ---: | ---: | ---: |
| 1 | 1 | 390.6 | 357.0 |
| 1 | 2 | 891.0 | 351.6 |
| 1 | 3 | 629.1 | 334.9 |
| 1 | 4 | 477.4 | 290.2 |
| 2 | 1 | 377.9 | 521.4 |
| 2 | 2 | 1666.1 | 560.9 |
| 2 | 3 | 1142.7 | 590.6 |
| 2 | 4 | 786.4 | 398.9 |
| 4 | 1 | 1089.1 | 1118.8 |
| 4 | 2 | 2182.5 | 975.2 |
| 4 | 3 | 1553.5 | 919.0 |
| 4 | 4 | 1029.6 | 679.6 |

Scale-2/3/4 conversion generally improves substantially. Native conversion is
not a claimed speedup: its hot loop remains unchanged, and the two-session
sample above regressed. A focused nine-pair repeat with 128 samples per trial
found median total CPU per native frame batch of 2,120.2 → 2,119.5 µs for one
session, 3,682.4 → 3,670.2 µs for two, and 13,539.6 → 13,746.2 µs for four.
Single-session baseline trial p50 values varied from 124.5 to 578.8 µs on this
shared workstation. Preserve both sets; this variance does not support a
universal native-loop improvement or a precise small regression estimate.

A five-pair scalar comparison disables GCC tree-loop and SLP vectorization for
the same candidate C source. With one session/eight participants, native full
frames measured 234.4 µs with vectorization versus 627.5 µs without. GCC reports
16-byte/8-byte vectorization in the native pixel loop and the generic scaled
inner sum, with alias-dependent loop versioning. Scale specialization supplies
constant box dimensions/divisors; it is not evidence that every specialized
loop became SIMD. Scaled scalar/vector comparisons vary by case. Retain the
compiler cost model, with no ISA-specific intrinsics, global -march=native,
forced SIMD pragma or global vectorization disable.

For overlapping rectangles, the one-session scale-2 median damage critical
section changed from 11.182 to 1.322 µs. At scale 2, 32 empty frames changed from
a median 284 voluntary context switches to three; median total CPU per frame
batch changed from 60.2 to 1.0 µs. Real capture already avoids conversion when
there are no rectangles; the empty case tests stale-mask/dispatch overhead,
not a claim that idle capture previously converted continuously.

Sparse work has a tradeoff. One scale-2/eight-participant case measured
19.8 → 40.0 µs conversion p50 while total CPU per frame batch changed from
86.1 to 46.6 µs. Four concurrent sessions measured 93.8 → 35.5 µs in the
corresponding sparse case. The policy favors low CPU/wakeup cost for small
updates; it does not promise lower wall latency for every individual update.

## Large pools and host budgets

The separate large-pool replay compares capacities 8/16/32/64/128 for 1080p,
4K and 8K source surfaces, one/four sessions, and sparse/full work. It records
actual pool capacity and jobs dispatched. Correctness at 128 threads is covered
by the normal tests, including releasing and joining the pool, and by matching
output checksums across capacities. This 32-logical-CPU host cannot establish
128-core scalability, NUMA locality or an optimal policy for a large server.

The large replay has 60 cases × three trials = 180 runs. Selected full-frame
results below use the same mean-session-p50 / median-trial aggregation as above;
CPU is total process milliseconds per frame batch across all sessions. Capacity
comparisons run in matrix order, without a paired old-code baseline, and remain
sensitive to clock/scheduling changes.

| Sessions | Source | Capacity | Active jobs/session | Conversion ms | Total CPU ms/batch |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | 3840×2160 | 8 | 8 | 1.809 | 13.550 |
| 1 | 3840×2160 | 32 | 32 | 1.659 | 14.202 |
| 1 | 3840×2160 | 128 | 32 | 1.835 | 16.231 |
| 1 | 7680×4320 | 8 | 8 | 9.142 | 65.248 |
| 1 | 7680×4320 | 32 | 32 | 8.001 | 75.109 |
| 1 | 7680×4320 | 128 | 127 | 8.152 | 81.787 |
| 4 | 3840×2160 | 8 | 8 | 6.294 | 75.161 |
| 4 | 3840×2160 | 32 | 32 | 5.657 | 75.041 |
| 4 | 3840×2160 | 128 | 32 | 5.665 | 78.854 |
| 4 | 7680×4320 | 8 | 8 | 23.634 | 371.599 |
| 4 | 7680×4320 | 32 | 32 | 21.220 | 398.857 |
| 4 | 7680×4320 | 128 | 127 | 21.250 | 459.270 |

More participants sometimes reduce wall time at a higher total CPU cost; 128
is a supported ceiling, not the measured optimum here. The production helper
on this 32-CPU host caps its capacity at 30, so the 64/128 cases deliberately
stress the module beyond its automatic local selection. Four 8K sessions also
exceed the 60 FPS conversion budget in this replay before encoding. No automatic
use of 128 threads on this host follows from raising the supported ceiling.

Affinity is a CPU eligibility set, not a cgroup CPU-time quota. Pools remain
independent per helper; subtracting two does not reserve two cores globally.
The work threshold limits waste for small surfaces, but it is not a global
multi-tablet CPU scheduler. Keep encoder/compositor work in physical system
measurements before selecting a shared hard quota. T382 is closed as `wont_fix`
for now: maintainer multi-tablet testing and a larger/NUMA-machine campaign
are outside the current scope. Users needing larger systems can tune affinity, NUMA
placement and cgroup limits and measure performance on their own hardware.
The supported 128-participant ceiling and its correctness coverage remain.

Performance controls should offer sensible defaults and explain their tradeoffs;
users decide which settings are worthwhile on their systems. Measurements guide
recommendations, while correctness and lifecycle tests establish supported use.
The helper measured on 2026-09-17 selected its capacity automatically. T474
subsequently added a Linux-only Auto / 1–128 participant control; see the
[current setting](../architecture.md#processes-and-settings). Its availability
does not require a large-machine benchmark.

## mmap, DMA-BUF and other boundaries

Changing malloc to anonymous mmap would not eliminate the present capture copy:
UScreen registers a userspace BGRA pointer with libevdi, and the inspected stock
EVDI GRABPIX path copies damaged pixels into that pointer. The current aligned
allocation plus MADV_HUGEPAGE is advice, not proof of huge-page backing or copy
removal. T389 compares page backing/faults and shared raw-frame transport.

A memfd/MAP_SHARED ring can change helper-to-encoder ownership and remove a
transport copy only when both sides share validated slots and retain them until
all readers finish. It does not automatically remove EVDI's initial copy,
conversion, FFmpeg plane copies or a GPU upload. Mapping the current FIFO is
not that protocol; the stock CLI/FIFO fallback remains.

DMA-BUF is promising for a different capture-to-hardware-encoder path when the
producer/exporter and importer agree on formats, modifiers, device access and
synchronization. The current public EVDI client path supplies CPU pixels, not
an exported captured-frame FD. CPU mapping of a DMA buffer also needs its
cache-coherency/access protocol. See the [kernel DMA-BUF contract](https://docs.kernel.org/driver-api/dma-buf.html).
This is T389 integration work, not a measured T383 optimization. No FFmpeg or
EVDI patch is required by the changes here.

io_uring concerns FIFO/socket submission and completion rather than pixel
arithmetic. Its separate T405 transport replay found no consistent advantage
at 60 FPS; readiness-based production I/O stays in place. A successful ring
setup does not establish a performance benefit or remove payload copies.

## Measurement boundaries and reproducibility

[`conversion.c`](../../scripts/benchmarks/conversion.c) uses public conversion
and frame-exchange APIs. It runs independently owned session pools in one
process, not full helper processes; no EVDI, compositor, encoder or Android
is opened. Source rows have 64 padding bytes. Six unmeasured rotations populate
all three output buffers and histories. Sparse work marks one chroma row per
64, overlap submits 63 heavily overlapping rectangles (below capture's
64-rectangle overflow fallback), and full work marks all rows.

Per-job timings include dispatch, pixel work and joining; damage timing is the
critical section and lock wait is measured separately. The mutex is uncontended
in this replay, so lock results do not establish live writer/capture contention.
Frame publication/claim/release is outside conversion timing but inside the
whole trial. CPU uses CLOCK_PROCESS_CPUTIME_ID across all process threads;
barriers exclude final checksumming. Setup/worker creation/teardown are outside
phase timing. Context switches are process deltas from getrusage. VmHWM reports
whole-process peak RSS, including setup and instrumentation; it is not the
virtual stack reservation or an isolated per-frame allocation measure.

The recorded logical byte count is source BGRA reads plus packed NV12 writes
for dirty rows: `rows × output_width × (8 × scale² + 3)`. Divide by conversion
time for logical throughput. It is not a memory-controller bandwidth counter:
caches, write allocation, library accesses and synchronization add different
traffic. No hardware DRAM bandwidth claim is made. Timing and sampled tails
are affected by frequency, thread placement and background desktop activity;
there is no fixed timing pass/fail threshold.

```sh
python3 scripts/benchmarks/conversion.py --baseline 02f71e2 --trials 3 --samples 32 --output /tmp/matrix.json
python3 scripts/benchmarks/conversion.py --quick --scalar --baseline 02f71e2 --trials 5 --samples 64 --output /tmp/vector.json
python3 scripts/benchmarks/conversion.py --native --baseline 02f71e2 --trials 9 --samples 128 --output /tmp/native.json
python3 scripts/benchmarks/conversion.py --large --trials 3 --samples 24 --output /tmp/large.json
```

The controller alternates variant order, compiles the baseline modules from
git without modifying the checkout, records source hashes, and verifies final
pixel checksums across variants/damage/worker counts. Compiler vectorization
background and diagnostic flags are described in [GCC's documentation](https://gcc.gnu.org/projects/tree-ssa/vectorization.html).
[Raw artifacts](2026-09-17-conversion/) include compressed JSON, source hashes,
compiler vectorization output, red/green regression logs and SHA256SUMS.

## Regression evidence

The normal Rust integration suite builds and executes the C fixtures: five
conversion tests, 28 helper regressions and the independent-module lifetime
test pass. The independent color oracle checks exact two-stage rounding,
odd source dimensions, padded strides, dirty/clean rows and output guards for
scales 1..4. It runs against sanitizer builds and actual -O3 scalar/vectorized
builds. Separate tests exercise clustered partitioning, 128 participants,
affinity restriction/high CPU IDs, failed worker creation, epoch wrap, mode
replacement, FIFO recovery and shutdown.

The new empty/tiny-dispatch, affinity and 128-capacity regressions failed before
their fixes; a further high-ID affinity case failed with a fixed cpu_set_t.
The same assertions pass afterward. T226's real FIFO recovery integration
passes with both CLI and in-process encoder builds; host Clippy passes with
warnings denied. The helper also builds and links against
stock libevdi with -O3 -Wall -Wextra -Werror. It was not attached to the running
desktop for this refactor.
