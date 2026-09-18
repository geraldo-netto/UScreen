# Frame admission and decoder output pacing — 2026-09-18

T399 keeps the existing capture cadence and synchronous output policy. Lower
input rates made the tablet's isolated decoder delay markedly worse. A bounded
decode-all/render-latest prototype remains disabled by default: it has not
shown a useful benefit on this tablet. Dropping compressed reference pictures
is not an acceptable substitute for controlling raw-frame admission.

## Cadence result

The same H.264 motion/static fixtures used by T386/T403 were replayed at
1/2/5/15/30/60 inputs per second, three trials each: 36 valid trials, three
seconds warmup and twelve seconds measurement. Configured codec rate remains
60 FPS; the pictures are replayed more slowly, not recaptured at each rate.
The Ulefone RugKing Pad 2 Pro uses `c2.unisoc.avc.decoder`, 1280×800, app brightness
0.5 and observed 60 Hz. USB charging remains attached. Exact APK/source and
fixture hashes are retained in the [artifacts](2026-09-18-frame-pacing/).

Values are medians of three per-trial metrics; CPU is percent of one core.
The clock interval begins immediately before decoder input admission and ends
at the local output-release note. It excludes capture, encoding, USB/ADB video
transport and physical display scanout.

| Inputs/s | Motion release p50 ms | Static release p50 ms | Motion CPU % | Static CPU % |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1016.81 | 1017.67 | 6.05 | 5.16 |
| 2 | 516.81 | 517.09 | 6.86 | 5.89 |
| 5 | 215.89 | 215.72 | 8.88 | 7.88 |
| 15 | 82.25 | 82.00 | 15.78 | 14.66 |
| 30 | 48.13 | 48.09 | 25.21 | 24.15 |
| 60 | 30.50 | 30.44 | 44.18 | 42.19 |

The approximately one-input-interval dependence is consistent with buffering
that needs subsequent input; these measurements do not identify its precise
native driver cause. Every valid trial reported zero invalidations/duplicate
notifications, with one final input still lacking a notification after a
750 ms no-input drain. Increasing the existing 200 ms idle keepalive would risk
longer stale-picture intervals. Reduced process CPU does not establish reduced
whole-device power or acceptable interactive latency.

## Decode-all/render-latest experiment

`DecodedOutputDrainer` optionally performs at most three zero-timeout lookahead
dequeues after the first available output. Only when a newer decoded output
exists does it release the previous output without display. It always submits
all compressed input in order. At most four outputs are considered per batch;
format changes, empty queues and retirement stop the lookahead. Native operations
remain within one `CodecLifetime` borrow, outside the receiver monitor.
Only presented sequences record output timing/receive render acknowledgements.
Discarded decoded outputs have a separate counter; the replay also records their
sequence identities. This experiment is synchronous only, with no new UI setting.

The 36 steady-rate trials compare legacy output versus lookahead at 5/60/90 FPS
for motion/static, with two seconds warmup, ten seconds measurement and three
alternating-order rounds. Both policies use the same APK and source cohort.
All reported zero discarded outputs, zero invalidations and zero duplicates.
All but the final input received notification. The panel remained at 60 Hz:
90 decoder callbacks/s is not evidence of 90 visible frames/s.

| Scene / rate | Output | Release p50 / p99 ms | CPU % |
| --- | --- | ---: | ---: |
| Motion / 5 | Legacy | 216.28 / 219.17 | 9.33 |
| Motion / 5 | Latest | 216.48 / 219.64 | 9.42 |
| Motion / 60 | Legacy | 30.65 / 33.47 | 44.07 |
| Motion / 60 | Latest | 30.92 / 33.99 | 45.49 |
| Motion / 90 | Legacy | 24.43 / 27.54 | 55.22 |
| Motion / 90 | Latest | 25.03 / 27.80 | 56.36 |
| Static / 60 | Legacy | 30.45 / 33.52 | 42.61 |
| Static / 60 | Latest | 30.75 / 33.69 | 44.20 |

The extra polling does not help when no newer decoded output is ready. The
slightly higher measured CPU/latency reinforces retaining the existing default;
this is one device and short trials, not a universal decoder result.

A further 24 valid trials deliver groups of six or twelve complete encoded
pictures at 60 source frames/s. Each group waits until its last picture would
be available, then admits its pictures in original order. A synthetic source
clock records the intended uniform source time separately from actual feed time;
these source-to-release intervals include the deliberately imposed delivery
stall. They are not real capture timestamps or a measured network simulation.

| Scene / batch | Legacy source→release p50 / p99 ms | Latest p50 / p99 ms |
| --- | ---: | ---: |
| Motion / 6 | 94.21 / 116.07 | 94.49 / 116.96 |
| Motion / 12 | 170.42 / 217.06 | 171.72 / 217.88 |
| Static / 6 | 93.89 / 116.05 | 93.54 / 116.62 |
| Static / 12 | 169.94 / 216.72 | 170.76 / 217.52 |

All 24 valid trials still discarded **zero** decoded outputs and notified
599/600 measured inputs, with zero invalidations/duplicates. Input/native decoder
backpressure did not produce a ready-output backlog that this policy could
shorten. The experiment therefore supplies no reason to enable output skipping,
even under these admission bursts. It does not rule out benefits on another
codec/device with an actual ready-output backlog.

## Whole-raw-frame admission and quality

A deterministic four-second, 60-FPS NV12 corpus contains text, pen strokes and
moving content. Before encoding, admit complete raw pictures at 60/30/15/5 FPS.
Stock FFmpeg/libx264 uses ultrafast, zerolatency, CRF 18, eight threads, no B
frames and wall-clock periodic IDRs. All twelve files decode to exactly their
admitted frame count, with keyframes at seconds 0, 1, 2 and 3. No coded reference
picture or partial raw FIFO frame is discarded.

Quality holds each decoded picture until the next admitted source picture and
compares it with the original 60-FPS NV12 timeline. Thus PSNR includes temporal
staleness as well as compression error. It is not a perceptual/text legibility
score, and the NV12 reference has already discarded RGB/chroma detail. The
compressed source corpus, generator/font metadata, exact commands, encoded
files and checksums are retained for reproduction and T400's codec comparison.

| Scene | Admitted FPS | Encoded bytes / 4 s | Held-picture PSNR dB |
| --- | ---: | ---: | ---: |
| Motion | 60 | 4,224,002 | 54.75 |
| Motion | 30 | 2,599,306 | 23.05 |
| Motion | 15 | 2,142,306 | 20.21 |
| Motion | 5 | 1,290,526 | 19.02 |
| Pen | 60 | 725,914 | 60.52 |
| Pen | 30 | 730,702 | 45.73 |
| Pen | 15 | 750,070 | 42.68 |
| Pen | 5 | 703,489 | 40.37 |
| Text | 60 | 640,171 | 60.86 |
| Text | 30 | 680,529 | 62.55 |
| Text | 15 | 694,092 | 62.66 |
| Text | 5 | 648,706 | 61.37 |

Motion saves encoded bytes at a large temporal-quality cost. Static/pen byte
counts are not monotonic with FPS at fixed CRF and one-second keyframes. These
are offline admission experiments, not evidence that a live pre-conversion
capture gate reduces CPU, saves battery or preserves idle-to-motion latency.

## Selected policy and recovery contracts

- Keep latest-raw-frame coalescing and the 200 ms unchanged-frame keepalive.
  Preserve the last pending damage even when no later EVDI dirty event arrives.
  A future pre-conversion gate must own stable BGRA storage and flush that final
  update; merely skipping the existing conversion can lose it. T418 tracks the
  separate shared-memory ownership work.
- Preserve whole FIFO transactions. Once any bytes of a raw frame are written,
  finish or retire/quarantine that writer generation. Never replace the remaining
  bytes with pixels from a newer picture. T226/T405 tests cover this contract.
- Keep encoded backlog recovery at an IDR with current CSD; do not drop arbitrary
  interdependent access units. Join/reconnect and sequence-based ACK identity
  retain their existing tests and behavior.
- Keep the ten-second transport read deadline and decoder watchdog. Longer idle
  policies would need to coordinate both, including the one-frame tail and
  recovery after an idle-to-motion transition. No longer idle policy is enabled.
- CLI keyframes use input wall-clock time; the optional in-process encoder uses
  admitted frame-count PTS/GOP plus explicit IDR requests. One-second recovery
  cannot be assumed equivalent after frame skipping. This experiment's enforced
  source-time keyframes are not proof of that unimplemented integration.

No adaptive FPS/scale policy is selected. T388 continues sustained physical
power controls with brightness preserved. This report's short battery snapshots
cannot resolve small changes against the gauge's approximately 9.99 mAh steps.
The [deployed baseline](2026-09-17-device-baseline.md) remains the full pipeline
comparison point; none of these isolated numbers replaces its packet-to-ACK
measurement or proves an overall playback speedup.

## Validation and source integrity

The normal Android suite includes ten API 27/34 drainer tests for unchanged
legacy behavior, latest-output release ordering, bounded batches, sparse/format
transitions and retirement; three pure pacing tests cover ordinary deadlines,
batched/partial groups and invalid bounds. The explicit plan runner rejects
invalid cadence/duration, duplicate identities and installed-APK/provenance
mismatches before launching a workload. Its permanent regressions were observed
failing before validation, then passing. The full working tree passed 254 Android tests with zero failures/errors/skips and lint, including four T400 fixture-parser tests. The separately exported T399-only index passed all 250 of its tests and lint. Exact logs are retained.

The cadence cohort uses the final T403 replay. The steady cohort adds output
lookahead; the final burst cohort also extracts drainer construction to keep
cyclomatic complexity at most nine and records synthetic source/discard clocks.
The source archives preserve each actual APK's input sources, including unused
T400 multi-codec fixture support in the latter cohorts. All T399 device inputs
are the original H.264 fixtures. Compare policies within a cohort, not CPU
differences between independently instrumented APKs.

The installed older host interrupted measurements by redelivering its token and
launching UScreen over the benchmark. Incomplete trials are excluded and retained
separately; remaining trials resume with the same verified APK. T420 tracks the
foreground-stealing behavior, and T421 tracks observed old-host DTS warnings.
Only relevant token-delivery log lines are archived, not unrelated Activity logs.
No installed host/main APK, Cinnamon process or EVDI module was replaced here.

Sequence-derived codec timestamps remain unsuitable as a monotonic render clock
(T414). Raw reported-render values are retained separately and not used as
physical latency evidence. The bounded render-latest implementation does not
correct that independent timestamp issue.

## Reproduction

Build the separate APK with `scripts/benchmarks/decoder-project.py`, install its
candidate package, and run `decoder-plan.py --serial SERIAL --plan PLAN.json
--provenance PROJECT/provenance.json --output NEW_DIRECTORY`. Each raw archive
contains the executed plan and exact controller source. The installed APK hash
must match. `summarize-decoder.py RAW --output SUMMARY` preserves trial boundaries;
the archived burst analysis additionally uses the synthetic source clock.
Backgrounding aborts a trial rather than continuing to seize foreground focus.

For whole-picture admission, decompress each corpus `.nv12.xz` into a temporary
corpus directory, copy `corpus-metadata.json` there as `metadata.json`, then run
`frame-admission.py --corpus DIRECTORY --output NEW_DIRECTORY`. It requires stock
FFmpeg/ffprobe and NumPy. The corpus generator additionally uses Pillow and the
recorded font. Compare the preserved input hashes before interpreting reruns.
