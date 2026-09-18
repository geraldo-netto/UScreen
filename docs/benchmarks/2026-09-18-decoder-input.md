# Android compressed-input transport — 2026-09-18

T403 retains the reusable heap/InputStream path as the production default.
Reading compressed packets directly into codec-owned buffers did not demonstrate
a useful latency advantage in the isolated tablet replay. The experimental path,
its cancellation contracts and permanent tests remain available for other devices.
No FFmpeg patch, host deployment or main Android APK replacement was required.

## Implementation and regression coverage

`ChannelPacketReader` owns a nonblocking SocketChannel and Selector. It stages
only a five-byte prefix, validates the existing packet size/type/sequence format,
and reads the payload into the caller's buffer. Config packets can have up to
four payload bytes in that prefix. The frame sequence remains unsigned 32-bit
metadata; CSD and frame flags are unchanged. A ten-second absolute deadline spans
the entire header/payload transaction, including codec-slot acquisition. It does
not rely on `Socket.soTimeout` to interrupt channel reads. Closing the reader
wakes selection and closes its channel; interruption and partial EOF fail input.
The deadline is rechecked after a native call returns; it cannot preempt a stuck
native codec call. T386 retirement retains that call's storage and bounds the
shutdown caller's wait independently.

`DecoderSession.feedDirect` borrows the T386 codec lifetime outside the receiver
monitor and permits only synchronous input. It checks slot capacity before
reading and requires the exact payload length before queueing. Retired owners
cannot queue a payload returned by a late transport read; their native storage
remains alive until that read returns. Callers must close the reader when the
session is retired. Complete-payload arrival remains the timing boundary used by
the staged decoder path. `VideoReceiver` does not select the experiment.

Permanent tests in the normal Android suite cover fragmented header/payload,
invalid lengths/types, truncated metadata and EOF, CSD and sequence wrap,
destination capacity, pending-payload ordering, absolute deadline, reader close,
blocked-read retirement and late fills. The initial shared input helper accepted
a short fill and could queue stale slot bytes. Its test was observed failing
before the exact-length check, then passing without queueing any partial input.

The separate replay's socket sender also exposed two cancellation gaps: receiver
rejection left the sender blocked, and Activity backgrounding could not interrupt
a write while native decoder input was stuck. Permanent API 27/34 regressions
were observed failing before their fixes. Terminal receive exit closes both
socket peers; the sender uses nonblocking writes with a ten-second absolute
deadline and checks Activity activity/interruption between writes. Backpressure
waits at most ten milliseconds before checking again. A worker never joins itself.
The test source set includes the replay input helpers, and CI builds the separate
replay APK so those sources cannot silently drift from their production contracts.

The final normal Android run passed **237 tests**, with zero failures/errors/skips,
and lint. These include host-side Robolectric API 27/34 lifecycle tests; physical
measurements below use Android 16/API 36. The benchmark-tooling suite also passed.

## Measurement boundary

The tablet, stock H.264 fixtures and codec profile match the
[T386 experiment](2026-09-18-decoder-profiles.md): Ulefone RugKing Pad 2 Pro,
`c2.unisoc.avc.decoder`, 1280×800, configured 60 FPS, legacy synchronous hints,
app-only 50% brightness and preferred/observed 60 Hz. USB and its charging route
remain attached. The separate candidate package owns both loopback TCP peers.
Each receiver requests a 128 KiB receive buffer and records the observed size.

Each cohort has three alternating-order trials of heap/direct input across
motion/static at 60/5 FPS: five seconds warmup, twenty seconds measurement and
up to 750 ms notification drain. The same already encoded access units are sent
at each selected rate; sparse motion is a slowed replay, not five-FPS capture.
Both paths use the same codec/profile/APK within a cohort.

The local feed timestamp precedes writing the framed access unit to loopback;
output release and local acknowledgement are correlated by sequence. Thus these
intervals include loopback admission, receipt and decoder work, but exclude host
capture/encoding, USB/Wi-Fi video transport and host ACK receipt. CPU and ART
allocation include both socket peers, tracing and codec Java work. Source payload
ByteBuffers, headers and timing bookkeeping still allocate; removing a staging
copy does not mean allocation-free processing.

As in T386, the reported render timestamp equals sequence-derived PTS rather
than a comparable monotonic clock. Negative feed-to-reported-render intervals
remain in the raw data and are not used as latency evidence. Release/notification
timestamps are distinct; neither establishes optical display latency.

## Results and source cohorts

The initial and reviewed cohorts each completed **24 trials**. All 48 recorded
zero decoder invalidations, duplicate notifications and transport errors. At
60 FPS, 1,199 of 1,200 measured inputs received a render notification; at 5 FPS,
99 of 100 did. Each trial's last input again lacked a notification after the
750 ms no-input drain. Direct reads used actual direct codec buffers for every
submitted packet: 1,501 at 60 FPS or 126 at 5 FPS, including warmup and CSD.
Both paths reported 131,072-byte receive buffers and a 60 Hz display.

The reviewed cohort uses the final cancellation/deadline fixes. Values below are
medians of three per-trial metrics, not pooled frame percentiles. Per-trial
ranges and the initial cohort remain separate in the artifacts.

| Scene / FPS | Input | CPU % of one core | ART MiB / 20 s | Feed → release p50 / p99 ms | Feed → ACK p99 ms |
| --- | --- | ---: | ---: | ---: | ---: |
| motion / 5 | direct | 9.06 | 2.07 | 217.73 / 220.93 | 221.43 |
| motion / 5 | heap | 8.94 | 2.09 | 217.25 / 220.52 | 221.13 |
| motion / 60 | direct | 50.32 | 3.70 | 31.66 / 34.68 | 35.27 |
| motion / 60 | heap | 50.05 | 3.59 | 31.52 / 34.54 | 35.16 |
| static / 5 | direct | 8.99 | 2.07 | 217.58 / 220.84 | 221.45 |
| static / 5 | heap | 8.85 | 2.09 | 216.82 / 220.13 | 220.71 |
| static / 60 | direct | 50.82 | 3.68 | 31.64 / 34.70 | 35.37 |
| static / 60 | heap | 49.42 | 3.56 | 31.37 / 34.29 | 34.87 |

The final motion/60 comparison is 50.05% versus 50.32% of one core, with feed-to-
release p99 34.54 versus 34.68 ms (heap/direct). Static/60 is 49.42% versus 50.82%
and 34.29 versus 34.70 ms. These results do not justify switching input defaults.
The initial cohort similarly showed 49.90% versus 50.93% CPU at motion/60 and
release p99 34.73 versus 34.95 ms. Sparse input remains roughly 217 ms, consistent
with T386's measured input-dependent delay; removing this copy does not fix it.

The initial benchmark sender used blocking writes and lacked terminal-receive
cleanup. The reviewed sender closes both peers on exit and bounds nonblocking
writes using Activity state and a deadline. The two complete cohorts are retained
rather than relabeling the earlier measurements as final-code measurements.

| Cohort | Candidate APK SHA-256 |
| --- | --- |
| Initial | `b44eafa55e02dbb829e103b7f1d66de301b99d219415aed4ba87e39c00711b04` |
| Reviewed | `ea2ded1075b7ce51fe6618596aa7068ee33e7a27bbf529ab0f2ec33f49debc83` |

## Selection and limits

The experiment removes the reused heap-payload-to-codec copy but provides no
evidence of a useful latency gain on this hardware. Keep the tested staged path.
No raw-buffer, mmap or DMA zero-copy claim follows from a direct ByteBuffer: the
network stack and codec implementation retain their own ownership and copies.

API 30's [LinearBlock](https://developer.android.com/reference/android/media/MediaCodec.LinearBlock)
offers a separate block-input model and a codec-specific copy-free compatibility
query. A positive query still does not guarantee allocation success; mapped memory
must not be used while queued or after recycling. This experiment does not
implement or measure that model. Any later adoption needs the API 27 fallback,
bounded block ownership and device measurements; a standard direct buffer does
not establish LinearBlock compatibility.

Twenty-second battery snapshots are too short to separate these paths on the
tablet's approximately 9.99 mAh charge-counter steps. CPU/copy counts are not
power measurements. T388 retains sustained controls; no battery-saving claim or
production input-mode switch is based on this replay.

## Reproduction and evidence

The [artifact directory](2026-09-18-decoder-input/) preserves both raw cohorts,
per-trial/group summaries, exact original/instrumented source snapshots, APK
provenance, red/green logs, test counts and SHA-256 checksums. Build products/APKs
are excluded. Fixture bytes and regeneration metadata are retained in the linked
T386 report; both cohorts record their matching fixture hashes.

With the Android SDK/JDK configured, build the separate replay package and use the
T386 fixture files:

```sh
python3 scripts/benchmarks/decoder-project.py --directory /tmp/decoder-input \
  --package com.uscreen.decoderbench.candidate
adb -s SERIAL install -r /tmp/decoder-input/app/build/outputs/apk/debug/app-debug.apk
python3 scripts/benchmarks/decoder-device.py --serial SERIAL \
  --motion /tmp/decoder-motion.bin --static /tmp/decoder-static.bin \
  --output /tmp/decoder-input-series --seconds 20 --warmup 5 --trials 3 \
  --variant candidate/socket-heap --variant candidate/socket-direct
python3 scripts/benchmarks/summarize-decoder.py /tmp/decoder-input-series \
  --output /tmp/decoder-input-summary
```

Backgrounding interrupts the benchmark and aborts further launches. Keep the
main UScreen package and host daemon unchanged when comparing these isolated
transport variants. Whole-project rollout and sustained power comparisons follow
the selected optimization batch.
