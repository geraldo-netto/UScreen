# Scheduling preference — T582, 2026-09-21

**High priority changes CPU allocation under contention, but this live sample
does not demonstrate lower video latency or a fix for YouTube audio gaps.**
High remains the default requested by the maintainer, with a Normal override.

## Live pipeline observation

The installed application remained unchanged during collection. Linux daemon
1160771, EVDI helper 1160852 and FFmpeg 1160959 shared `uscreen.service` throughout;
all 90 threads observed at setup belonged to that group. The GUI and same-user
ADB server were moved from the editor's group to individual UScreen scopes.
Only these three groups changed weight. No EVDI restart, ADB reset, Android
lifecycle action or application replacement occurred.

Six runs used weights **100, 1000, 1000, 100, 100, 1000**. Each allowed five
seconds after changing weights, then observed 25 seconds. No builds or deliberate
CPU stress ran during these observations. Desktop content was not controlled;
this is an alternating passive observation, not matched video replay. Configured
capture remained 1280×800/30 FPS, stock H.264 VAAPI Baseline through FIFO.

Latency is **encoded-packet ready to Android render-callback ACK received by
the host**. It excludes capture/encoding and optical presentation, and measures
no audio latency. The values below are medians of the existing approximately
five-second window percentiles, not percentiles of pooled per-frame samples.

| Pair | Normal p50 / p95 | High p50 / p95 |
| --- | ---: | ---: |
| 1 | 17.80 / 22.75 ms | 17.90 / 22.35 ms |
| 2, reversed order | 17.80 / 22.90 ms | 17.90 / 23.60 ms |
| 3 | 17.70 / 23.00 ms | 17.70 / 25.50 ms |
| Median of runs | **17.80 / 22.90 ms** | **17.90 / 23.60 ms** |

There were 649 acknowledged samples in Normal and 656 in High, with no aged-out
samples reported. Encoded delivery during the observed content was approximately
9.1–9.3 access units/s; the configured 30 FPS is a limit, not proof that this
workload continuously supplied 30 different pictures/s. No dropped-frame or
YouTube playback-quality conclusion is drawn from these counts.

Combined daemon/helper/FFmpeg CPU medians were **2.16% versus 2.19% of one core**.
Summed runnable-queue delays across their retained threads were just
0.63–5.30 ms per 25-second run. New/exited threads are excluded from these deltas.
These already-small delays offer little evidence of CPU starvation in this
sample. Every PipeWire snapshot reported **ERR=0**; existing audio data threads
kept SCHED_RR priority 20. The intermittent stutter was not reproduced, so these
observations neither identify its cause nor prove that higher priority fixes it.

## Controlled CPU contention

A separate test used two always-runnable Python workers pinned to logical CPU
31, in sibling user scopes. Each trial lasted four seconds; a start barrier
aligned both workers. The real UScreen pipeline's affinity was never changed.
Three Normal and three High trials used the same alternating order as above.

| Subject / competitor weight | Subject share of paired CPU time, three trials |
| --- | --- |
| 100 / 100 | 50.0%, 50.0%, 50.0% |
| 1000 / 100 | 90.8%, 90.8%, 90.8% |

This validates scheduler behavior under controlled contention. It is not a
UScreen throughput, video-latency, GPU or audio improvement measurement.

## Implementation and verification

The [scheduling policy](../../scheduling.md) defaults to High in the shared
configuration, with isolated Linux, Windows and Darwin adapters. Linux uses
fair-scheduler CPU weight; Windows sets the process class and explicitly carries
it into shared command launchers; Darwin requests process nice -5 and reports
permission denial. No application requests real-time scheduling.

Permanent tests cover defaults, Normal persistence/merging, the GUI selector,
group ownership, actual native child/thread inheritance, an isolated fake ADB
server, denied commands, delayed scope activation and bounded invalid inputs.
The shadowed-ADB regression first reproduced missing elevation when a bundled executable hid the running SDK server; discovery now checks known ADB executables throughout PATH. Another ADB regression initially failed because a side effect inside a disabled
`tracing::info!` argument never ran; it passes with the operation outside logging.

- Common library: **100 passed**; GUI: **68 passed**.
- Host binary: **365 passed**, three existing ignored tests retained.
- Service/path packaging tests: **5 passed**.
- Linux scheduling scope: **17/17 production functions meet 80% line coverage**.
- GUI priority control: **15/16 executable lines, 93.75%**.
- Windows workspace/tests cross-check passed. Standalone Darwin
  commands/scheduling tests and portable wasm policy cross-checks passed.
- Clippy passed with the existing T573 parity-warning allowance; complexity
  remains at most 9.

Windows/macOS native execution and coverage remain **T583**, not passing results.
Darwin's existing Linux-only command-test observation helper also needs T584.
The full macOS application backend remains unavailable.

High weight was restored for all live UScreen groups after testing. The service
override persists; GUI/ADB scopes last for their current processes. Installed
binaries were not replaced, so automatic priority setup and the new UI require
a build containing T582. No performance benefit is inferred from the new default.

`summary.json` preserves parsed window metrics and thread deltas.
`metadata.json` identifies the installed artifact and evidence checksum.
`evidence.tar.gz` retains original journals, PipeWire samples, scheduling
snapshots, synthetic worker results and the exact one-off collectors. Collector
PIDs are historical identifiers, not commands to rerun against future processes.
