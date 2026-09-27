# T597: exact bounded latency summaries

Latency reporting uses an in-place hybrid: sort windows of at most 64 samples,
retain already sorted windows, reverse descending windows, and select p95 then
p50 within its lower partition for other windows. Maximum scans only the upper
tail. The reporting cadence, 1,024-sample cap, storage reuse, nearest-index
rounding, milliseconds and separate host/tablet clock intervals are unchanged.
Empty windows remain absent; saturated u32 samples retain their exact meaning.
No allocation or shared lock is added to summarization.

Permanent bounded property tests compare against the previous sort-based oracle
for every length from zero through 1,025, across random, duplicate, ascending and
descending data. Additional cases cover singletons, small-window boundaries,
2,048 samples, zero and u32 saturation. They also check sample preservation and
reusable capacity. Existing log/clock/loss regressions remain in the normal suite.
This is an algorithm change, with no claimed prior behavioral defect.

The [runnable benchmark](../../scripts/benchmarks/latency-percentiles.rs) imports
the actual production helper. Median nanoseconds per call on this Linux host,
including identical buffer reset, five repetitions of 50,000 calls:

| Samples | Distribution | Previous sort | Hybrid |
| --- | --- | ---: | ---: |
| 16 | random | 34.6 | 25.8 |
| 16 | sorted | 10.7 | 16.3 |
| 64 | random | 176.2 | 201.5 |
| 256 | random | 909.1 | 694.0 |
| 1,024 | random | 4,454.3 | 2,181.7 |
| 1,024 | eight-value duplicates | 2,161.3 | 1,827.9 |
| 1,024 | sorted | 280.8 | 157.2 |
| 1,024 | reverse sorted | 321.5 | 355.5 |

The constant overhead can lose a few nanoseconds on small or descending windows;
there is no universal speedup. At roughly five-second reporting intervals even
the full-window gain is microseconds, not evidence of improved frame latency,
audio playback, battery consumption or total pipeline throughput.

Fresh native collection passed 204 Linux library tests, 383 Linux binary tests
and 15 Windows latency tests as ordinary `BLENT-WIN11\blentdev`. Three existing
opt-in binary microbenchmarks remain ignored by the normal suite; no regression
was skipped. Combined native counters pass all 33 scoped production functions,
individually at least 80%, with no missing counters. Complexity: 6,408 functions,
none above nine. Formatting and whitespace checks pass.

[Retained evidence](artifacts/2026-09-27-latency-summary/) includes raw native
LCOV, source hashes/snapshot, exact tests, coverage report, benchmark samples and
host/compiler identity. Windows source attestation retains the complete guest
hash list plus the subset named by its LCOV; uncompiled Linux tray files in that
scratch checkout are not Windows evidence. Linux counters have their own source
snapshot. These are scoped counters, not a new whole-project coverage claim.
