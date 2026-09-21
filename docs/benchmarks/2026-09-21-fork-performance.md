# Performance work since the fork

Evidence inventory through 2026-09-21. The local upstream merge base is
`96fce89c2b3e57e092d59a4890235aec9ae777c3` (upstream 1.2.3); the first fork
commit is `6fc24c5`, dated 2026-09-16. The comparisons below come from the fork's
recorded experiments, each against its own stated baseline. They are not one
matched upstream-versus-current benchmark, and their percentages must not be
added or multiplied into an overall speedup.

**The clearest measured tablet benefit is the low-latency VAAPI H.264 profile:**
raw-input-to-render-ACK median fell from 35.74 to 19.04 ms for motion and from
222.39 to 23.05 ms for sparse text in the matched USB replay. The latest shared
capture work separately reduced conversion-start-to-render-ACK median by 2.6%
with the optional software encoder. Neither measurement starts at desktop
compositing or ends at optical presentation.

Unless stated otherwise, reductions are `(before - after) / before`; throughput
increases are `(after - before) / before`. Rounded source values may slightly
change the last decimal. Source reports retain workloads, repetitions, hashes,
raw observations and regression evidence. “Implemented” describes repository
behavior; experiments are explicitly separated below.

## Measurements involving the physical tablet

Host: Ryzen 9 7945HX / RX 6600 XT. Tablet: Ulefone RugKing Pad 2 Pro,
`c2.unisoc.avc.decoder`. These rows concern 1280×800 streams on this device.

| Change | Before | After | Measured gain | Scope and tradeoff |
| --- | ---: | ---: | --- | --- |
| VAAPI High to Constrained Baseline, motion at 60 FPS — T400/T479 | 35.74 ms | 19.04 ms | **46.7% lower median** | Raw NV12 write, encoding, USB, decoder callback and return ACK. Motion bytes grew about **23.3%**; decoded quality was effectively equal in this cohort. [Matched USB replay](2026-09-18-profile-selection.md#combined-raw-writerender-ack-results). |
| Same profile change, sparse text at 5 FPS — T400/T479 | 222.39 ms | 23.05 ms | **89.6% lower median** | Same boundary and cohort. Removes this decoder's observed extra-picture delay; not a universal Android result. [Source](2026-09-18-profile-selection.md#combined-raw-writerender-ack-results). |
| Same profile change, decoder-only motion at 60 FPS — T400 | 27.77 ms | 11.03 ms | **60.3% lower median** | Feed to output release, excluding USB/host encoding; same improvement measured at a narrower boundary, not an additional saving. At 5 FPS: 211.81 to 11.87 ms, **94.4% lower**. [Source](2026-09-18-codecs.md#physical-decoding-and-profile-selection). |
| FIFO to leased shared input, libx264 at 60 FPS — T418 | 17.8300 ms p50; 21.1069 ms p95 | 17.3733 ms p50; 20.5486 ms p95 | **2.6% lower p50 and p95**; about **0.457 ms** median saved | Conversion start through render-ACK. Five alternating pairs, both metrics improved in all five; all 7,200 frames acknowledged, encoded files identical. Default only for optional in-process libx264. Installed CLI VAAPI still uses FIFO. [Source](2026-09-21-shared-capture/README.md#before-and-after). |

T479 also implements bounded, receipt-verified automatic encoder/decoder
comparison. The table demonstrates the tested profile's benefit; it does not
claim a separate speedup for the selection algorithm. Manual preferences remain
authoritative. The [September 21 activation](2026-09-21-shared-capture/README.md#installed-update-and-live-acceptance-t565)
verified the installed applications at the user's **30 FPS**, VAAPI Baseline
setting. That live observation is not a matched repetition of the 60 FPS trials.

## Host pipeline and computation

These are measured stages or synthetic replays. Large percentages here do not
mean the whole application, video call or tablet became that much faster.

| Change | Before | After | Measured gain | Scope and tradeoff |
| --- | ---: | ---: | --- | --- |
| VAAPI processing depth 2 to 1 — T400 | H.264 17.826 ms; HEVC 17.795 ms | H.264 3.273 ms; HEVC 3.188 ms | **81.6% / 82.1% lower p50** | Raw write to packet output, 1280×800 motion/60 FPS. Capability-gated stock FFmpeg option; same checksums/timestamps. Four H.264 encoders: 18.340 to 5.151 ms, **71.9% lower** in separate concurrency series. [Source](2026-09-18-codecs.md#paced-encoder-output). |
| Publish complete FFmpeg packets without waiting for the next picture — T448 | H.264 VAAPI 18.46 ms at 60 FPS; 203.70 ms at 5 FPS | 1.77 ms; 3.65 ms | **90.4% / 98.2% lower p50** | 640×400 raw-input-to-packet-ready, one stream. Both sides already use depth one. Across codecs/1–4 streams, removes approximately one input interval. No tablet in this experiment. [Source](2026-09-18-packet-framing.md#method-and-results). |
| Shared input's host conversion/encoding stage — T418 | 1.2955 ms | 0.9387 ms | **27.5% lower p50** | Same five-pair experiment as the 2.6% tablet row, ending earlier at encoded-packet readiness. Do not count both as separate overall gains. [Source](2026-09-21-shared-capture/README.md#before-and-after). |
| Convert horizontal damage spans — T554 | 64×64 patch: 26.29 µs; narrow strip: 119.13 µs | 3.78 µs; 9.73 µs | **85.6% / 91.8% less conversion time** | 1280×800, scale 1, eight-participant pool. 640×360 window CPU: 232.03 to 97.57 µs, **57.9% lower**. Full-screen changes do not gain pixel savings; scale-2 full-frame time regressed 8.1%. FIFO capture benefits; shared-slot full conversion remains separate T570 work. [Source](2026-09-20-damage-regions/README.md#isolated-measurements). |
| Specialized scale-2/3/4 conversion — T383 | 891.0 / 629.1 / 477.4 µs | 351.6 / 334.9 / 290.2 µs | **60.5% / 46.8% / 39.2% lower** | Full 1920×1080 source, one session, eight participants. Native scale-1 loop has no established general gain. [Source](2026-09-17-conversion.md#measured-conversion). |
| Dirty-mask merging and work-sized dispatch — T383 | Overlap marking 11.182 µs; empty-batch CPU 60.2 µs | 1.322 µs; 1.0 µs | **88.2% less marking time**; empty-fixture CPU **98.3% lower** | Empty fixture context switches: 284 to 3. Real capture already skipped clean conversion. One sparse case cut CPU 86.1 to 46.6 µs but increased wall time 19.8 to 40.0 µs. [Source](2026-09-17-conversion.md#measured-conversion). |
| Incremental Annex B scan — T384 | H.264 fragmented 2.762 ms; large NAL 519.052 ms | 0.125 ms; 4.787 ms | **95.5% / 99.1% shorter replay** | Synthetic parser replay. Fragmented allocations 19,191 to 390 (**98.0% fewer**); dense H.264 time 1.182 to 0.761 ms (**35.6% lower**). Later T448 replaces legacy picture-boundary waiting; these remain historical parser-stage measurements. [H.264 and HEVC cases](../benchmarks.md#annex-b-packetizer-replay). |
| Read directly into bounded CLI assembly storage — T407 | Single-stream H.264: 2.396 / 9.212 / 49.586 ms CPU | 2.186 / 8.005 / 31.714 ms CPU | **8.8% / 13.1% / 36.0% less CPU** | Fragmented/dense/large-NAL read replays, after T384. Across all cases: 4–13% fragmented/dense and 36–38% large-NAL. Large-NAL explicit copies 105.0 to 42.0 MB (**60.0% fewer bytes**). Not a live encoding measurement. [Source](2026-09-17-cli-assembly.md#measured-result). |
| Retain known encoded packet buffers — T402 | 64 KiB: 1.657 µs; 512 KiB: 11.354 µs | 0.719 µs; 3.500 µs | **56.6% / 69.2% less CPU** | Optional in-process packet allocation/fill/publication, zero older outputs retained. Full 64 KiB–4 MiB matrix saves 43–69%; small packets retain copy fallback and show no gain. Encoding itself excluded. [Source](2026-09-17-packet-storage.md#measured-result). |
| Fill writable codec planes directly — T389 | 4K: 189.9 raw frames/s; p99 9.795 ms | 278.6 raw frames/s; p99 6.306 ms | **46.7% more raw throughput**, **35.6% lower p99** | Optional in-process FIFO path, saturated single stream without encoding. Four streams: 285.6 to 448.7 aggregate frames/s (**57.1% more**). Removes a staging copy for contiguous NV12; padded fallback has no consistent gain. [Source](2026-09-18-raw-input.md#results). |
| Immediate raw FIFO readiness — T405 | Small-frame p99 5.965 ms; large-frame p99 21.081 ms | 1.993 ms; 5.361 ms | **66.6% / 74.6% lower transfer tail** | One paced stream, optional Rust reader plus native writer, 1.5/8.2 MB frames. Excludes capture/encoding/tablet. Four large streams use about **27.8% more CPU** while reducing the transfer tail. [Source](2026-09-17-readiness.md#paced-transfer-results). |
| Remove empty-FIFO polling — T405 | 487 reads/s; 2.485 ms CPU; 0.809 ms stop | 1 read/s; 0.191 ms CPU; 0.023 ms stop | **99.8% fewer reads**, **92.3% less CPU** in fixture | One idle optional reader for one second; cancellation median **97.2% lower**. EOF/no-writer fallback and other session counts retained separately. [Source](2026-09-17-readiness.md#idle-states-and-cancellation). |
| Notify capture-buffer retirement instead of sleeping — T389 | 486 / 993 / 342 µs after release | 7 / 18 / 25 µs after release | **98.6% / 98.2% / 92.7% lower completion delay** | Held leases of 10/100/900 ms. 11/97/856 polling sleeps become one condition wait; one-second timeout remains bounded. Scheduler-dependent native replay. [Source](2026-09-18-raw-input.md#results). |

## Bookkeeping, resource use and tools

| Change | Before | After | Measured gain or removed work | Boundary |
| --- | ---: | ---: | --- | --- |
| Native pen/touch/pointer batching — T406 | 256 complete writes | 38 complete writes | **85.2% fewer writes** | Same 256 events and 38 synchronization frames, byte-for-byte identical. Counting sink; no measured physical pen-latency improvement. [Source](2026-09-17-input-batching.md). |
| Host sequential ACK lookup — T404 | Last entry 76.52 ns; missing 76.72 ns | 4.30 ns; 3.32 ns | **94.4% / 95.7% lower lookup time** | One tracker, bounded 256-entry release-build replay. Sparse fallback worsens 75.72 to 98.85 ns. Full ACK/network handling excluded. [Source](2026-09-17-timing.md#rust-lookup-and-report-allocation-replay). |
| Reuse host latency report storage — T404 | 200 allocations + 800 reallocations | 0 + 0 after warmup | **100% fewer measured allocator calls** | 100 report windows, one tracker; logging subscriber excluded. Capacity growth/concurrent reports can still allocate. [Source](2026-09-17-timing.md#rust-lookup-and-report-allocation-replay). |
| Android timing cache — T404 | Corrected scan: 31.05 / 58.32 ns | Corrected cache: 24.50 / 23.52 ns | **21.1% / 59.7% faster delayed lookup iteration** | Eight/32 arrivals behind, one host JVM session. Latest/colliding lookup adds 1.29/6.65 ns. Not Android ART, FPS or render latency. [Source](2026-09-17-timing.md#android-lookup-replay). |
| Reuse video send deque — T391 | 10,001 allocations + 20,000 reallocations | 1 + 0 | Removes **30,000 allocator calls** | 10,000 eight-packet batches; requested bytes 10,400,024 to 24. Not allocation-free encoding/networking. [Source](2026-09-17-stream-resources.md). |
| Publish one framed picture per batch — T416/T448 | 87,380 pictures; 12,582,912 bytes batch vector | 1 picture; 384 bytes vector | **99.997% less peak batch-vector storage** | Extreme six-byte synthetic units. Bounds pre-admission metadata; tiny packets add one allocation each. Not process RSS or normal picture sizes. [Source](2026-09-18-packet-framing.md#publication-batches-and-metadata-t416). |
| Bound slow-viewer storage and retention — T391 | No equivalent byte budget; stalled viewer retained until shutdown | 32 MiB admitted backing/session; one-second write/batch deadline | New enforced resource bounds | Four sessions: at most 128 MiB admitted backing. No measured RSS reduction or stable general latency gain; other allocations excluded. [Source](2026-09-17-stream-resources.md). |
| Isolate tablet discovery/recovery — T390 | Ready devices wait for stalled probe; 250 ms test deadline fails | 12.44 / 12.37 / 18.26 ms connection medians | Removes cross-device blocking | 1/2/4 ready fake devices plus one indefinitely stalled probe. Old run does not complete, so no valid percentage speedup. [Source](2026-09-17-discovery.md). |
| Cache GUI process/tool status — T409 | 30 inventories; 30 capability groups; 30 idle ADB queries | 0 inventories; 6 groups; 0 idle ADB queries | **100% / 80% / 100% fewer fixture probes** | 30 virtual polls with stable daemon/no assigned sessions. Full identity checks remain. Idle periodic repaint requests fall from one/s to zero. [Source](2026-09-18-status-polling.md#controlled-probe-counts). |
| Filter process discovery — T409 | 1,000 full snapshots | 100 general snapshots, or 2 daemon snapshots + 100 name reads | **90% / 99.8% fewer full snapshot attempts** | Controlled 1,000-process inventory with 100 same-user entries; not a corresponding speedup factor. [Source](2026-09-18-status-polling.md#controlled-probe-counts). |
| Bounded reusable fake-tablet receive buffers — T408 | Large-fragment CPU 525.59 ms; traced peak 6,146.7 KiB | 12.50 ms; 68.2 KiB | **97.6% less receiver CPU**, **98.9% less traced peak allocation** | Development measurement client only. Full 1/2/4-client matrix CPU savings 89.4–97.6%; no production Android gain. [Source](2026-09-17-fake-tablet.md#results). |

Other implemented improvements have contract or correctness evidence without
a matched performance percentage: moving filesystem persistence off Tokio
workers (`c64fe94`), creating input devices only for attached tablets (`6fc24c5`),
[bounded Android codec retirement](2026-09-18-decoder-profiles.md#lifetime-corrections),
owned cancellation and resource cleanup, and configurable conversion capacity
up to 128 participants. Capacity support does not establish 128-core scaling.
The control-load replay [T387](../benchmarks.md#android-control-replay) supplies
queue/recovery evidence, not a before/after speedup.

## Experiments and changes that are not established production gains

| Work | Observed result | Current interpretation |
| --- | --- | --- |
| Adaptive idle — T492 | Isolated raw writes: 5 to 2/s, 7.68 to 3.072 MB/s (**60% fewer**) | Opt-in guarded production policy exists; full-pipeline native acceptance remains open. The earlier writer experiment did not test that complete policy. [Writer measurement](2026-09-20-idle-writer/README.md), [implemented policy](../reviews/2026-09-21-reliability-batch.md#item-2--adaptive-idle-capture). |
| Sparse H.264 battery replay — T419/T492 | Local five-to-one update/s replay improved net charging current by **40.1 mA** on average | Two balanced rounds, USB attached; not absolute power, battery-life gain or live two-update/s behavior. [Source](2026-09-19-presentation-power.md#results). |
| Android callback decoding — T386 | Sparse CPU 7.85% to 4.48% of one core, but motion p99 33.91 to 37.23 ms and allocations 3.92 to 46.90 MiB | Synchronous default retained; no consistent latency advantage. [Assessment and source](../reviews/2026-09-20-android-performance.md#existing-measured-experiments). |
| Direct socket reads into Android codec buffers — T403 | Motion p99 34.54 ms heap versus 34.68 ms direct | No useful gain; reusable heap path retained. [Source](2026-09-18-decoder-input.md#results-and-source-cohorts). |
| Decode-all/render-latest — T399 | Motion/60 FPS p99 33.47 ms legacy versus 33.99 ms candidate; no ready outputs discarded | Experimental policy remains disabled. [Source](2026-09-18-frame-pacing.md). |
| io_uring and larger FIFO capacity — T405/T389 | Mixed CPU/tail results; large queues can retain much older frames | No automatic switch justified. Isolated capacity study is not evidence for changing current backend settings. [io_uring](2026-09-17-io-uring.md), [capacity](2026-09-17-pipe-capacity.md). |
| RGB rectangle compression — T419 | About 33% less app CPU for pen replay, but inconsistent battery outcome | Research renderer; hardware H.264 retained for measured battery-focused setup. [Source](2026-09-19-presentation-power.md). |
| Bundled FFmpeg 6.1.6 — T563 | Version, codec availability, packaging and live operation verified | **No matched speedup measured** for the version change. [Source](2026-09-21-ffmpeg6/README.md). |
| Battery-saver controls / lower configured FPS — T388 | User can choose resource/quality tradeoffs | No controlled current-app normal-versus-saver battery benefit established. Current 30 FPS must not be compared with old 60 FPS CPU as a code speedup. [Source](2026-09-18-power-policy.md). |

The early shared-memory prototype's much larger raw-transfer gains are already
covered by the later T418 production-boundary measurement; they are not an
additional current display gain. Likewise, codec labels alone do not establish
a winner: the [codec experiments](2026-09-18-codecs.md) retain AV1 software
latency, HEVC interoperability failures and compression/quality tradeoffs.

## Attribution and limits

- The inherited [upstream benchmarks](../benchmarks.md#host-cpu), including
  approximately 190% to 97% pipeline CPU, 280% to 12% FFmpeg CPU, huge-page capture
  gains and Wi-Fi-lock results, predate this fork. They are excluded from our gains.
- The September 17 [full-device baseline](2026-09-17-device-baseline.md) already
  includes early fork changes. Later live observations use changed profiles,
  FPS, content and software; they do not establish a matched total improvement
  since the fork. No overall CPU, battery-life or optical-latency percentage is
  available.
- Most host studies ran on an active shared workstation; retain source reports'
  repeated-run ranges and regressions alongside the selected improvements.
- Some dated reports still describe their original implementation/deployment
  state as current. T574 tracks that wording drift. This inventory uses current
  implementation/activation status while preserving historical measurements.
