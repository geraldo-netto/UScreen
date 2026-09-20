# T492: isolated writer and keyframe cadence

The 2 FPS candidate reduces steady idle raw-frame writes by **60%**, while
fresh publications still wake the writer at its configured 30/60 FPS pacing.
This establishes host-side feasibility, not tablet latency or battery savings.
Production remains at five idle updates per second; no 2 FPS application
option is implemented or enabled by this experiment.

## Experiment and results

The driver uses the actual C frame exchange, writer and nonblocking FIFO writer
with synthetic 1280×800 NV12 frames. It never opens EVDI, touches ADB or changes
the Android Activity. The two candidate binaries compile a temporary copy of
`writer.c` with its interval changed from 200 to 500 ms; production source is
unchanged. Stock FFmpeg receives the same timestamp filters, framing and codec
options as UScreen, exported through the shared Rust encoder policy.

Three variants were compared: current 200 ms/one-second keyframe expression;
500 ms with the existing expression; and 500 ms with a 0.9-second keyframe
threshold. The last is an experimental coordination candidate, not a complete
profile-admission or recovery implementation. QP/CRF is 18, configured bitrate
20,000 kbps, at nominal 30 and 60 FPS. VAAPI CQP does not enforce that bitrate
ceiling. This differs from the live preference of 60,000 kbps.

Each variant has a six-second static trial and a 7.5-second mixed trial. Mixed
content publishes just before 500 ms boundaries, one second of motion, and a
fresh frame following roughly three seconds of idle content. Images are flat
luma fields with neutral chroma and unique publication markers. These simple
images test timing/ownership; their encoding cost and compressed size are not
representative of YouTube or a desktop. Trials run once per matrix cell.

| Encoder executable and profile | Trials | Current maximum IDR gap | Plain 2 FPS maximum | Earlier deadline maximum |
| --- | ---: | ---: | ---: | ---: |
| System FFmpeg 6.1.1; libx264 and VAAPI Baseline | 24 | 1201.39 ms | 1500.41 ms | 1412.44 ms |
| Exact live AppImage FFmpeg 5.1.9; VAAPI Baseline | 12 | 1200.51 ms | 1501.66 ms | 1084.00 ms |

The first pass used the system executable, so the VAAPI matrix was repeated
with the running AppImage's wrapper and stock executable. Its SHA-256 is
`0dafc1360bb07743f76abeb1e4ae16b0aaa1331e9041a0e8d6a8524111dc0cbb`.
The system executable hash is
`ed16af623947494a72e284b6eb8ff225f2da22b38b5d5069c2fd4b4ba3384e41`.
This version comparison is not a ranking of codec speed: there are no repeated,
balanced latency/quality trials or controlled representative video content.

Across all 36 trials, **1,551 complete input frames produced 1,551 packets**.
All **236 flagged keyframes** decoded independently with their current headers
to a complete 1280×800 picture. Packet positions and sizes were independently
checked with ffprobe. No fresh publication was absent from writer output.
The final packet arrived within 4.49 ms of its raw-write start. Shutdown waits
for a complete writer lease before closing input; deliberate partial-frame
cancellation belongs to the existing quarantine regressions, not this trial.

During seconds 1–5 of every static trial, current cadence wrote 20 frames and
the candidates wrote eight. At 1,536,000 bytes/frame this is 7.68 versus 3.072
MB/s of raw writer traffic: **60% fewer bytes/encodes for unchanged content**.
It is not a measured 60% reduction in total CPU, USB bandwidth or power.
The largest fresh-publication-to-write delay was 33.384 ms at a 30 FPS target
and 16.674 ms at 60 FPS in the system matrix. The writer remains subject to
normal frame pacing; reducing idle repeats did not introduce a 500 ms wakeup
delay in these samples. The earlier keyframe request is quantized/evaluated
only when input reaches FFmpeg; its nominal 0.9 seconds is not a hard wall-time
guarantee, as the 1.412-second startup interval demonstrates.

## Remaining admission and validation

`host/src/selection/worker.rs::choose` marks a selection verified after render
progress. That flag does not certify sparse-stream latency. Explicit encoder
choices also do not select a named decoder through this automatic path. Thus
a checkbox guarded only by `Selected.verified`, or a hard-coded codec-name
allowlist, would claim evidence the implementation does not possess.

T492 remains open for a host-controlled option backed by current, scoped
sparse-profile measurement and conservative fallback on profile, software,
stream or peer-epoch changes. Timing controls must be portable policy with
backend-specific application; the optional in-process encoder has a separate
IDR mechanism and must retain its compatibility cadence until validated.
Keep the 200 ms production default until this is implemented. The earlier
proposal to expose an opt-in control was therefore not enabled prematurely.

Still required: permanent failing tests before those runtime changes, including
fresh damage/deadline rounding, immediate wake, final packet flush, late joins,
slow clients, long pauses/reconnects, in-process IDR requests, Android watchdogs
and selection health. Independently decoding a packet here establishes a join
point, not successful production-client reconnect. Full capture/encode/USB/
presentation and sustained battery measurements need a suitable device window
without deliberately reattaching the live EVDI output through unresolved T222.
No Android/GUI/daemon/capture binary was changed or restarted for this experiment.

## Reproduction and checks

```sh
python3 scripts/benchmarks/idle-writer.py target/idle-study \
  --encoders libx264 h264_vaapi_baseline
# To use the installed image's encoder, prepend its extracted usr/bin to PATH.
```

`system-*` and `bundled-*` JSON files summarize both passes. `evidence.tar.gz`
retains commands, raw writer/packet timing, streams, probes, candidate source
copies and scripts. The first-pass script copy matches its recorded hash;
the later script adds source/tool provenance without changing the experiment.
SHA256SUMS covers the retained artifacts. Existing production idle/framing
regressions passed: nine host idle tests, ten framing tests (one pre-existing
opt-in benchmark ignored), and three C T405 tests. All 110 Python benchmark
tests passed. These tests preserve current contracts; they are not evidence
that the unimplemented lower-cadence application policy has passed them.
