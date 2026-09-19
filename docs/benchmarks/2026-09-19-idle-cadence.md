# T492 — live sparse H.264 checks

**Keep the production 200 ms idle interval until cadence and recovery are
coordinated.** One-update/s Constrained Baseline remains responsive on this
tablet, but its encoded keyframe gaps reach two seconds. H.264 High additionally
incurs about one second of raw-input-to-render-callback-ACK delay. A global
constant change would sacrifice recovery and responsiveness for an unverified
live-pipeline battery saving.

The preceding [battery comparison](2026-09-19-presentation-power.md) found
about 40.1 mA more net charging with one versus five local H.264 updates/s.
This follow-up checks whether actual encoding and USB delivery preserve the
necessary timing properties. It does not repeat the battery experiment or
measure EVDI capture or optical display latency.

## Experiment

Eight trials compare the production VAAPI H.264 High and Constrained Baseline
profiles at one and five updates/s. Two rounds reverse the four-case order.
Each trial contains two 12-second scheduled input phases with a controlled
decoder restart between them: **16 encoder/decoder sessions** in total.
The last input is submitted one period before the scheduled phase duration,
then the encoder drains. Post-warmup timing excludes the first second of
inputs; missing callbacks remain missing.

The unchanged synthetic text corpus is 1280×800 NV12, advertised as 60 FPS,
QP 18, limited-range BT.709. `profile-usb.py` exports encoder options from the
current shared Rust policy and runs stock FFmpeg with the production timing
filter, wall-time IDR expression and length/checksum-framed tee output. Only
input cadence changes; no FFmpeg patch is involved.

An isolated ADB reverse socket carries encoded access units to a separate
APK built from current production decoder sources, with timing instrumentation.
Both requested profiles use `c2.unisoc.avc.decoder`, operating rate 120 and
no standard low-latency hint. Every session verifies the exact decoder
configuration receipt. The tablet remains at 50% window brightness and 60 Hz.
Touch or leaving the app cancels the experiment. The maintainer authorized
this short test window; Linux keyboard focus is not an admission condition.

This path includes raw-write admission, live encoding, USB delivery, decoding,
callback notification and return ACK. It bypasses the EVDI helper, production
authentication/stream queue and physical presentation measurement. Its decoder
restart starts another encoder too; it is **not** a late-client join test on an
already-running stream. Keyframe availability below measures a prerequisite
for that recovery, not observed full reconnect duration.

## Results

Timing entries are the median of the four phase-level percentiles in each
case. They are not pooled-frame percentiles. Raw-input admission to ACK uses
one host monotonic clock, including both USB directions.

| Encoder profile | Inputs/s | Median p50 (ms) | Median p95 (ms) | Longest keyframe gap (ms) | ACKs / encoded pictures |
| --- | ---: | ---: | ---: | ---: | ---: |
| Constrained Baseline | 5 | 23.24 | 27.08 | 1200.06 | 240 / 240 |
| Constrained Baseline | 1 | 25.82 | 28.60 | 2000.77 | 48 / 48 |
| High | 5 | 223.53 | 228.38 | 1200.91 | 236 / 240 |
| High | 1 | 1026.75 | 1029.01 | 2000.60 | 44 / 48 |

Baseline's roughly 2.6 ms p50 difference is small in this short comparison.
High adds about 803 ms when input slows from five to one update/s. High also
leaves one picture without an ACK at the end of each phase, including the
750 ms final callback-drain window; Baseline acknowledges every picture.
These are callback observations, not compositor or optical measurements.
Baseline's first raw-write-to-ACK interval is 122–140 ms across the sessions,
including encoder startup. High's first interval is 223–226 ms at five
updates/s and 1023–1028 ms at one update/s. Those startup observations are
separate from the post-warmup percentiles above.

All 576 input pictures were encoded. The 568 received ACKs exclude the eight
unacknowledged High pictures. Packet probes verify byte positions and lengths
against observed tee packets before identifying actual independent pictures.
All 175 flagged key packets contain IDR, SPS and PPS NAL units. Their four
distinct payloads also decode independently to complete 1280×800 pictures
with stock software FFmpeg; this supports the join-point interpretation
without claiming a measured production reconnect.
Every one-update/s phase exhibits a roughly two-second keyframe interval;
five-update/s phases stay at approximately 1.2 seconds or less.

The initial Baseline one-update/s phase, for example, has packet PTS 0, 0.9,
1.9 seconds; the second picture cannot satisfy a one-second-from-previous-IDR
condition. More boundary misses appear later in the preserved stream. The
precise arithmetic/scheduling contribution to each later miss was not isolated;
future regressions must include timestamps just before, at and after the
deadline. An IDR rule evaluated only when input arrives cannot create a picture
between those arrivals.

## Code and regression review

- `host/evdi/writer.c` waits on the frame-exchange condition until its idle
  deadline. Publication wakes it immediately, subject to the existing target
  FPS pacing. `last_write_ms` updates on fresh frames too: after lengthening
  the interval, fresh damage just before an IDR opportunity could move the
  next unchanged-frame repeat further away. This mixed-damage case still needs
  a permanent regression before changing the writer.
- `host/src/capture/cli_encoder.rs` forces an IDR when an input PTS reaches
  `prev_forced_t + 1`. The existing T116 test checks software idle join points
  below 1.6 seconds with input every 200 ms. The observed one-update/s gaps
  would not meet that recovery budget. T448 already verifies sparse H.264
  packet publication without waiting for another input or EOF; the old pending
  final-NAL defect is not a reason to retain the current idle interval.
- `host/src/encoder.rs` uses frame-count GOPs in the optional in-process path;
  clients request an IDR on the next supplied frame. A reduced helper cadence
  also affects that request's wait. A CLI-only solution must not silently
  change this backend's recovery contract.
- Android's `DecoderOutputWatchdog` requires four queued inputs and over
  1.5 seconds without output before declaring a stall. Regular one-second
  output can be healthy, but that is not a latency guarantee. Its sparse-stall
  and codec-retirement regressions remain relevant. The production socket
  read timeout is 10 seconds; the isolated USB probe uses three seconds.
- Automatic-selection health uses pending rendered output and a six-second
  observation window; idle content alone does not trigger recovery. Slow
  rendering can remain "healthy", so health must not substitute for a measured
  profile/cadence admission rule. Production stream write deadlines and IDR
  backlog recovery also remain part of the implementation check.

## Implementation follow-up

T492 stays actionable in `TODO.md`; this investigation does not enable a new
production cadence or remove its unimplemented requirements.

1. Coordinate idle capture opportunities and CLI IDR scheduling with an
   explicit wall-time recovery budget. Cover startup, timestamp rounding,
   fresh damage just before a deadline, long pauses, reconnect and overload.
   Preserve immediate damage wakeup, bounded queues and completed-packet flush.
2. Automatically reduce cadence only for a verified encoder/decoder/stream profile,
   with a conservative fallback for unknown or changed selections. The current
   tablet supports Baseline as a candidate; it does not justify enabling the
   same cadence for High or other unmeasured codecs/devices. Keep any user
   control on the host and separate it from the motion FPS target. Preserve
   explicit user choices with documented latency/recovery tradeoffs.
3. Validate the optional backend, Android watchdogs, selection health and
   production late-client/slow-client recovery with permanent automated tests
   before changing runtime policy.
4. Measure fresh-damage-to-presentation behavior and sustained battery flow
   through the complete live pipeline. Use a safe existing capture setup;
   unresolved T222 forbids deliberately reattaching EVDI on the active desktop.
   Do not transfer the local replay's 40.1 mA gain to production as a promise.

## Evidence and reproduction

The [evidence bundle](2026-09-19-idle-cadence/README.md) retains the exact plan,
APK/source provenance, corpus identity, policy export, FFmpeg commands and
encoded streams, host timestamps, ACKs, decoder results, probes and test logs.
`summarize-idle-cadence.py` produces the joined timing and keyframe summary.
The corpus itself and build outputs are omitted; source and input hashes are
retained. Each phase's encoded stream is included so keyframe claims can be
checked independently.

The benchmark duration/rate extension has permanent admission coverage for
one-update/s 12-second phases, invalid values and corpus overrun; it failed
before the harness change and passed afterward. Separate analyzer tests reject
mismatched packet positions/counts, missing initial keyframes and backward
clocks, and preserve a two-second join-point gap instead of substituting the
one-second input interval.

Validation passed: 89 Python benchmark tests; 16 focused Rust/C tests covering
idle waits, retirement, FIFO backpressure, existing idle join points, startup
and packet flush, selection health and stream deadlines; 24 Android watchdog
test cases; APK build; and the complexity gate (3,954 functions, none above
nine). These preserve evidence about the current implementation, not a claim
that an unimplemented one-update/s capture policy passed those regressions.

The temporary candidate APK was removed, the tablet returned to production
UScreen, and only the normal 8890/8891 ADB reverse routes remain. The host
daemon, capture helper, FFmpeg, Xorg and Cinnamon remained running. Production
settings and the 200 ms idle interval are unchanged.
