# T587: allocation sizing and bounded Android growth

The Android display reader now caps growth at its maximum legal packet size,
8 MiB plus one byte. It retains the 512 KiB initial buffer and 50% growth below
that cap. For a maximum-sized first packet, requested array capacity falls
from 12,582,913 to 8,388,609 bytes: exactly 4 MiB less unusable capacity.
This is an array-sizing result, not a measured process-RSS or latency gain.

## Current allocation inventory

| Path | Current policy | Assessment |
| --- | --- | --- |
| `host/src/framed_annex_b.rs` | Validates complete packet length, reserves exact required growth, then reuses payload capacity. | Known-size allocation is already implemented. Large-packet capacity persists with the parser. |
| `host/src/annex_b.rs` | Preallocates combined configuration/picture length when known; access-unit assembly still copies and grows while collecting slices. | Replacing every Vec with an exact-sized allocation would ignore unknown-size assembly and ownership. |
| `host/src/ivf.rs` | Validates frame length and allocates a frame-sized owned Vec. | Size is precise; per-frame ownership still allocates. A pool needs final-consumer lifetime evidence. |
| `host/src/media_storage.rs` | Accounts for full backing capacity and preserves charges across slices. | Small views cannot evade the retained-byte budget. |
| `host/src/encoder_storage.rs` | Optional in-process encoder allocates requested bytes plus mandatory FFmpeg padding for large packets; small packets retain the stock allocator/pool. | Padding is required, not speculative Vec headroom. Keep this optional path distinct from default framing. |
| `host/evdi/frame_exchange.c`, `raw_ring.c` | NV12 storage derives from dimensions and buffer/slot counts; dirty metadata derives from geometry. | Predominantly fixed geometry and concurrent ownership, not geometric growth. |
| `host/evdi/capture.c` | ARGB capture buffer rounds up to 2 MiB alignment. | At 1280x800, logical padding is 98,304 bytes; at 2960x1848, 1,188,352 bytes. Any alignment change needs native page/fault/CPU evidence. |
| Android `VideoPacketReader.kt` | One connection-owned array, borrowed synchronous slices, bounded geometric growth. | T587 removes capacity above the protocol maximum; reuse and packet bytes are preserved. |
| Android `CameraWire.kt` | Copies each encoded camera packet into an exact-sized ByteArray. | Precise size does not imply low allocation rate. Pooling requires measurement and ownership analysis. |
| Android `ChannelPacketReader.kt` | Experimental direct-input path with a five-byte prefix buffer. | Remains experimental; earlier direct-input work did not establish useful gains. |

## Why the cap does not add allocations

Below the maximum, the new growth result equals the old result. If the old
result would exceed the maximum, the new result covers every possible future
legal packet. Therefore the cap cannot introduce another growth allocation
for the same input sequence. No old payload needs copying during replacement:
the synchronous reader has finished dispatching it before the next read.
Oversized lengths are still rejected before payload allocation.

The permanent `t587_growthNeverReservesUnusableProtocolCapacity` regression in
`VideoPacketReaderTest.kt` failed on API 27 and 34 before the fix. It feeds a
6 MiB payload, the largest legal frame, then a small frame; checks sequence,
length and payload endpoints; proves the same backing array serves all three;
and requires capacity to equal the largest legal packet. Existing malformed,
oversized, fragmented and truncated packet regressions remain intact.

## Deterministic policy comparison

Run `python3 scripts/benchmarks/allocation-sizing.py`. This model counts array
requests, not ART allocations, GC pauses, copies, allocator overhead or RSS.
Every case includes the initial 512 KiB allocation.

| Workload | Old growth allocations | Capped growth allocations | Exact growth allocations |
| --- | ---: | ---: | ---: |
| 300 x 64 KiB | 1 | 1 | 1 |
| 6 MiB, maximum, then small | 2 | 2 | 3 |
| Maximum, then small | 2 | 2 | 2 |
| 64 KiB increments from above initial capacity to maximum | 8 | 8 | 122 |

In the gradual synthetic sequence, requested array bytes total 29,720,583 with
old growth, 27,590,663 with capped growth, and 539,754,617 with exact growth.
These inputs illustrate the tradeoff; they are not an observed tablet trace.
The benchmark model's `maximum_resize_overlap` includes the replaced and new
array together, but cannot predict additional garbage awaiting collection.

## Current Rust replay

The retained T416 release-mode replay was rerun on the current Blent source
using Rust 1.90.0 in Debian 12. The table selects the single-session current
framing path, with three repeats per workload. Counters cover the calling
thread's Rust allocations; harness/queue metadata is included, and native
FFmpeg/C, Android and allocator-internal copy traffic are excluded.

| Workload | Allocations | Reallocations | Requested bytes | Explicit copy bytes |
| --- | ---: | ---: | ---: | ---: |
| 87,381 minimum packets | 349,545 | 0 | 45,966,102 | 524,286 |
| 1,024 x 512-byte packets | 3,093 | 0 | 1,495,144 | 524,288 |
| 4 x 1 MiB packets | 33 | 0 | 5,772,584 | 4,194,304 |

All repeats had identical counters. These synthetic marker packets establish
allocation behavior, not decode throughput or interactive latency. The full
existing harness also replays isolated synthetic consumers concurrently; no
physical multi-tablet campaign was run or reopened.

After the cap, all 520 Android unit tests and lint passed; all 559 measured
production functions met the 80% coverage gate. Complexity checked 5,609
functions with none above nine. Native tablet deployment/profile evidence remains separate: USB and
ADB did not enumerate a tablet during this audit. Further pool/trim decisions
need current native allocation stacks/rates, GC and matched latency evidence;
T589 records that prerequisite. Do not infer a leak from retained capacity or
claim battery/latency improvements from these counts.

Evidence: [sizing model](artifacts/2026-09-26-allocation-sizing/sizing.json),
[Rust replay](artifacts/2026-09-26-allocation-sizing/rust-replay.log.gz), and
[regression failure](artifacts/2026-09-26-allocation-sizing/red.log.gz).

## Native follow-up after tablet connection

The signed Blent release was installed and verified against its APK hash. Saved
motion snapshots report total PSS of 68,082 KiB before app restart and 69,595 KiB
after restart/warmup; these are separate observations, not a controlled comparison.
The immediate startup snapshot is 9,640 KiB before steady streaming. Java/native
heap and total RSS values are retained in the [deployment evidence](artifacts/2026-09-26-blent-deployment/).

These samples do not identify allocation frequency, object retention, GC pause
cost, or the receiver array's current capacity. No memory or latency improvement
is inferred. The signed release is non-debuggable and lacks shell profiling
opt-in; T589 now tracks an instrumented variant and fixed workloads rather than
a missing-device blocker. Further pool/trim policy changes remain evidence-led.
