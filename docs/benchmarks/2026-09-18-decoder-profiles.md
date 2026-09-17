# Android decoder scheduling and lifetime — 2026-09-18

T386 keeps the synchronous compatibility profile as the default. The experimental
callback owner reduces sparse-stream polling work on the measured tablet, but
has not demonstrated a latency improvement. Available host CPU and memory do not
change that selection criterion. The default path gains bounded retirement and
ownership checks without switching its hints, output priority or scheduling mode.

## Lifetime corrections

Input dequeue previously ran while holding the receiver monitor. A blocked native
call prevented Surface/stop handling from acquiring it. Input/output operations now
borrow a codec owner outside that monitor. Retirement closes admission immediately,
detaches session state and waits at most 500 ms for a separate cleanup worker.
Native stop/release occurs only after borrowed operations finish; a stuck driver
retains its own native storage rather than freeing storage still in use.

An initial receiver-local retirement guard did not cover Activity recreation.
`CodecLifetime` now registers pending retirement process-wide, before starting its
cleanup worker, and removes the registration only after native destruction returns.
`DecoderSession.setupCodec` rejects allocation while that registry is nonempty.
Recreating receivers therefore cannot accumulate codecs behind a stuck native
operation. This does not impose one active codec across the process: the registry
tracks retirement, not every already admitted active session. Codec startup itself
still uses the receiver monitor.

Output progress is also revalidated after sampling the clock. A retired output
worker cannot update a replacement codec's watchdog or statistics. Timing epochs,
sequence identity, CSD and reconnect/keyframe recovery retain their existing roles.

Permanent normal-suite tests include:

- `DecoderSchedulingTest.t386_inputWaitDoesNotHoldReceiverMonitor`: pause native
  input; another thread must acquire the receiver monitor.
- `t386_retirementDoesNotWaitForBlockedNativeInput`: retirement returns within
  750 ms while the paused native borrower still prevents native release.
- `t386_recreatedReceiverWaitsForRetiredNativeOwner`: a fresh receiver allocates
  no codec until the old borrowed operation and native cleanup finish, then can
  allocate exactly once.
- `DecoderWatchdogTest`: forced old-output/replacement interleaving cannot revive
  the new watchdog. Existing stall/fallback cases remain in the suite.
- `CallbackDecoderTest`: owned payloads and sequence/config order, no synchronous
  dequeue, retired callbacks, two-access-unit admission, producer cancellation and
  the absolute 200 ms pending-input deadline.
- `DecoderConfigurationTest`: supported/unsupported/unknown capabilities, API 27
  behavior, unchanged legacy keys and watchdog hint fallback.
- `scripts/tests/test_benchmark_decoder.py`: selected variants must match recorded
  provenance and the trials actually executed. Included by the normal benchmark
  tooling test discovery.

The monitor, watchdog, recreation and provenance regressions were observed failing
before their corresponding fixes. The final Android run passed **211 tests**, with
zero failures, errors or skips, plus lint. API 27 and 34 run under Robolectric;
this is lifecycle/contract coverage, not physical decoder certification.

## Experimental profiles

| Profile | Scheduling | Hints | Thread priority |
| --- | --- | --- | --- |
| baseline/legacy | Original synchronous owner | Existing standard/vendor keys and 2x rate | MAX output thread |
| candidate/legacy | Borrowed lifetime, synchronous | Same as baseline | MAX output thread |
| candidate/sync-normal | Borrowed lifetime, synchronous | Same as baseline | Normal output thread |
| candidate/callback-legacy | Handler callbacks | Same legacy requests | Normal callback Handler |
| candidate/callback-supported | Handler callbacks | Advertised standard low latency; supported 2x rate; no vendor key | Normal callback Handler |
| candidate/callback-supported1 | Handler callbacks | Advertised standard low latency; supported 1x rate; no vendor key | Normal callback Handler |
| candidate/callback-unhinted | Handler callbacks | No performance hints | Normal callback Handler |

All profiles retain the output watchdog; fallback overrides requested hints. The
experimental callback mailbox owns at most two copied access units including one
being submitted. A full mailbox blocks admission within its deadline; it never
silently drops an encoded reference frame. Expiry invalidates the decoder and
reconnects for fresh headers/keyframe. Detached copies are necessary because the
network reader reuses its input storage. T403 examines an alternative input path.

Android requires asynchronous callbacks to be registered before configuration and
forbids synchronous dequeue in asynchronous mode. The profiles follow those
[MediaCodec contracts](https://developer.android.com/reference/android/media/MediaCodec).
An [operating-rate request](https://developer.android.com/reference/android/media/MediaFormat#KEY_OPERATING_RATE)
is resource planning, not measured throughput. Capability queries and accepted
configuration do not establish effective low latency.

## Device and replay boundary

- Ulefone RugKing Pad 2 Pro, Android 16 / API 36, serial `8002RH1010011900`.
- Fingerprint: `Ulefone/GQ8002RH1_EEA/GQ8002RH1:16/BP2A.250605.031.A3/1782987754:user/release-keys`.
- Decoder: `c2.unisoc.avc.decoder`, reported hardware accelerated, advertised
  maximum instances 10. This is not a measured concurrent-session limit.
- At 1280×800 and configured 60 FPS, the decoder advertises 120 FPS size/rate
  support and does not advertise the standard low-latency feature. Neither fact
  establishes actual throughput or latency.
- Separate debug packages `com.uscreen.decoderbench.baseline` and
  `com.uscreen.decoderbench.candidate`; the installed `com.uscreen` APK and Linux
  daemon were not replaced. No desktop/display-manager restart was performed.
- App-only brightness 0.5 and preferred display rate 60 Hz, USB attached. Every
  result records observed display rate and before/after battery snapshots.
- Stock host FFmpeg 6.1.1 creates 360-frame H.264 clips using libx264 veryfast,
  zerolatency, CRF 18, GOP 60, no B frames, repeated headers and AUD. Motion is
  `testsrc2`; static is a fixed grid. No FFmpeg modification.
- Replays send complete access units at 60 or 5 FPS with configured stream rate
  still 60. Five-FPS motion replays the same pictures more slowly; it is an
  isolated sparse-input experiment, not an end-to-end five-FPS capture trace.
- Each trial has 5 s warmup, 30 s measurement and up to 750 ms callback drain.
  Profiles alternate order between cases/rounds. Fixtures are copied before each
  launch and the tablet-side hash must match before measurement.

This measures decoder submission, native output and notification work with already
encoded, in-memory access units. It excludes Linux capture, encoding, TCP, Wi-Fi,
ADB video forwarding and the real host ACK round trip. Process CPU is a percentage
of one core. ART allocations include the harness, timing instrumentation and codec
Java work. Native dequeue counts and durations are instrumented call counts and
elapsed time, not inferred scheduler wakeups. Context switches cover only thread
IDs present at both snapshots; new/retired threads are counted separately. No
system-wide scheduler trace or isolated app energy measurement is claimed.

## Source cohorts and interrupted trial

Baseline production decoder source is commit
`8c3279bc804f371783b48e1fa1983a13e3172c4d`. Candidate sources are preserved verbatim
and hashed alongside their instrumented copies. The first profile series used the
candidate before the process-wide Activity-recreation guard. The later correlated
trace series uses the final lifetime fix. Their statistics are kept separate.

| Cohort | Baseline APK SHA-256 | Candidate APK SHA-256 |
| --- | --- | --- |
| Profile series | `d550e0a542d481807479c83677ddea3b5dc7dad492826c4e5f00cedd14c11f4e` | `e02cc90c558b42a45644406336fbf9b63c7fb2fc43aca2e66c72c86a1b387048` |
| Correlated traces | `5e869d697aa8e3bce79a7ff2b500beb703b76aada2d3d348b4e3c990387364c7` | `cf9f55301c1d80289f6d0eeb49f43faf5cb62aaee5a6ab2cd186e25b3c798354` |

After 72 completed trials, `static-60-2-candidate-callback-supported` lost foreground
at 2026-09-17 23:00 UTC, recording 1,280 sent / 1,279 notified frames and zero codec
invalidations. The Activity marked the run incomplete and returned to UScreen.
The actor causing the focus change is unknown; it is not classified as a decoder
failure. That partial trial is retained under `interrupted/` and excluded from
comparisons. After the user confirmed the tablet was free, the same installed APKs
resumed the remaining planned trials, including a fresh complete replacement.
The archive retains the interruption and explicit resume plan. All raw host times
use UTC; this report's date is the local Europe/Rome date.

## Measurements

The profile series completed **84 valid trials**: three trials for each of seven
profiles across motion/static and 60/5 FPS. Every valid trial recorded zero codec
invalidations and zero duplicate notifications. Sixty-FPS trials notified
1,799/1,800 submitted frames; five-FPS trials notified 149/150. The final input did
not acquire a notification during the no-input drain; notification count is not
an optical display-frame count.

The following values are medians of the three per-trial statistics, not pooled
frame percentiles. The full seven-profile table and per-trial ranges are retained
in the artifacts.

| Scene / input FPS | Profile | CPU % of one core | Callback p50 / p99 ms | Output dequeue calls/s |
| --- | --- | ---: | ---: | ---: |
| motion / 5 | baseline/legacy | 7.45 | 216.37 / 219.33 | 93.57 |
| motion / 5 | candidate/callback-unhinted | 4.48 | 217.86 / 220.94 | 0.00 |
| motion / 5 | candidate/legacy | 7.85 | 216.56 / 219.97 | 92.77 |
| motion / 60 | baseline/legacy | 40.20 | 30.76 / 33.85 | 119.79 |
| motion / 60 | candidate/callback-unhinted | 41.63 | 31.83 / 37.23 | 0.00 |
| motion / 60 | candidate/legacy | 41.89 | 30.92 / 33.91 | 119.67 |
| static / 5 | baseline/legacy | 7.41 | 216.15 / 219.18 | 93.46 |
| static / 5 | candidate/callback-unhinted | 4.40 | 217.51 / 220.56 | 0.00 |
| static / 5 | candidate/legacy | 7.85 | 216.39 / 219.86 | 92.90 |
| static / 60 | baseline/legacy | 40.46 | 30.68 / 33.50 | 119.79 |
| static / 60 | candidate/callback-unhinted | 41.02 | 31.54 / 37.00 | 0.00 |
| static / 60 | candidate/legacy | 41.80 | 30.85 / 33.94 | 119.74 |

Removing polling eliminates synchronous dequeue calls in callback mode and
reduces sparse-process CPU from about 7.4–7.9% of one core to 4.4–4.5%. Matched
surviving-thread context switches fall from roughly 11,800–11,900 to 3,150 per
30-second sparse phase. These are not scheduler wakeup counts or battery savings.
The synchronous output thread spends about 98.5–98.9% of sparse-phase elapsed time
inside dequeue calls, mostly waiting rather than consuming CPU.

For motion at 60 FPS, callback-unhinted uses 41.63% of one core versus 41.89% for
the candidate synchronous default; callback p99 increases from 33.91 to 37.23 ms.
Callback mode allocates about 46.90 MiB of ART heap per motion phase versus
3.92 MiB for the candidate synchronous path, principally because mailbox payloads
must be detached. Static payloads reduce that difference (5.04 versus 4.01 MiB).
More available memory does not by itself remove allocation/GC latency costs.

The original synchronous source used 40.20% of one core in motion versus 41.89%
for the candidate with lifetime protection, and 7.45% versus 7.85% at sparse motion.
This is a measured CPU cost of the candidate in this cohort; the safety correction
is not described as a CPU optimization. Callback p50 remains close (30.76 versus
30.92 ms at 60 FPS), with no established display-latency gain.

Neither supported 1x/2x operating-rate hints, the legacy hint set nor normal
synchronous output-thread priority demonstrated a useful latency advantage on this
hardware. Their full results remain available for future devices/profile policy.

### Correlated trace series

The final source completed **36 trace trials**: three rounds of baseline legacy,
candidate legacy and callback-unhinted across the same four cases. This shorter
series uses 3 s warmup and 10 s measurement, with the same 750 ms drain. Each result
reports 60 Hz, zero invalidations and zero duplicate notifications. Counts were
599/600 at 60 FPS and 49/50 at 5 FPS. Three per-trial medians/percentiles are again
summarized by their median; the shorter series is not pooled with 30-second runs.

| Scene / FPS | Profile | Feed → release p50 / p99 ms | Feed → local ACK p50 ms |
| --- | --- | ---: | ---: |
| motion / 5 | baseline/legacy | 215.82 / 218.98 | 216.36 |
| motion / 5 | candidate/callback-unhinted | 217.92 / 220.34 | 218.31 |
| motion / 5 | candidate/legacy | 215.24 / 218.98 | 215.91 |
| motion / 60 | baseline/legacy | 30.42 / 33.28 | 30.96 |
| motion / 60 | candidate/callback-unhinted | 31.39 / 34.36 | 31.88 |
| motion / 60 | candidate/legacy | 30.22 / 33.47 | 30.82 |
| static / 5 | baseline/legacy | 215.61 / 218.71 | 216.20 |
| static / 5 | candidate/callback-unhinted | 217.64 / 220.25 | 218.18 |
| static / 5 | candidate/legacy | 216.11 / 218.84 | 216.62 |
| static / 60 | baseline/legacy | 30.22 / 32.80 | 30.73 |
| static / 60 | candidate/callback-unhinted | 31.15 / 34.50 | 31.69 |
| static / 60 | candidate/legacy | 30.34 / 33.31 | 30.93 |

The sparse delay already exists before output release: about 215–218 ms versus
30–31 ms at 60 FPS. Across 11,664 notified trace frames, median release-note to
callback-delivery time was 0.443 ms. Together with the last frame remaining
without notification during the drain, this is consistent with input-dependent
buffering. It does not identify a particular driver/parser cause or establish
optical latency. T399 records this risk before changing idle keepalive intervals.

Every one of those 11,664 framework-reported render timestamps equalled
`sequence * 1000`, rather than a comparable monotonic render time. Consequently,
all feed-to-reported-render intervals were negative; the summary retains them and
their counts as invalid timing evidence. No physical-render latency is computed
from them. The boolean `releaseOutputBuffer(index, true)` API propagates the input
PTS to the Surface timestamp; the project currently uses PTS for sequence identity.
T414 records the need to compare explicit monotonic release timestamps while
preserving ACK identity. The observed callback argument alone cannot distinguish
platform reporting behavior from presentation-timestamp handling.



## Interpretation limits

A render notification is not an optical timestamp. Android explicitly allows
[OnFrameRenderedListener callbacks](https://developer.android.com/reference/android/media/MediaCodec.OnFrameRenderedListener)
to be delayed and batched. Correlated traces retain feed time, output-release note,
framework-reported render time, callback delivery and local ACK event separately.
Missing and negative intervals must be reported rather than discarded. A release
note is sampled after the release call; it is not the compositor's scanout time.

The battery gauge advances in roughly 9.99 mAh steps; one 35-second phase cannot
rank these profiles by power. USB charging state and net battery trend do not
isolate decoder energy. T388 requires longer matched controls. API 27/34 unit
coverage and one API 36 hardware decoder do not justify a device-wide default
switch. T382 retains broader hardware/API and multi-tablet measurements.

## Reproduction and retained evidence

The [artifact directory](2026-09-18-decoder-profiles/) contains checksums, both raw
trial archives, per-trial/group summaries, complete seven-profile tables, fixture
bytes, all four original/instrumented source snapshots, APK hashes and red/green
validation logs. Build outputs/APKs are not committed. `sources.tgz` preserves
Gradle manifests/configuration and decoder sources; use the repository's matching
Gradle wrapper. Each candidate provenance manifest hashes the exact source used.

For a new working-tree replay, with Android SDK and JDK configured:

```sh
python3 scripts/benchmarks/decoder-project.py --directory /tmp/decoder-baseline \
  --package com.uscreen.decoderbench.baseline --revision 8c3279bc804f371783b48e1fa1983a13e3172c4d
python3 scripts/benchmarks/decoder-project.py --directory /tmp/decoder-candidate \
  --package com.uscreen.decoderbench.candidate
adb -s SERIAL install -r /tmp/decoder-baseline/app/build/outputs/apk/debug/app-debug.apk
adb -s SERIAL install -r /tmp/decoder-candidate/app/build/outputs/apk/debug/app-debug.apk
gzip -dc docs/benchmarks/2026-09-18-decoder-profiles/motion.bin.gz > /tmp/decoder-motion.bin
gzip -dc docs/benchmarks/2026-09-18-decoder-profiles/static.bin.gz > /tmp/decoder-static.bin
python3 scripts/benchmarks/decoder-device.py --serial SERIAL \
  --motion /tmp/decoder-motion.bin --static /tmp/decoder-static.bin \
  --output /tmp/decoder-series --seconds 10 --warmup 3 --trials 3 \
  --variant baseline/legacy --variant candidate/legacy --variant candidate/callback-unhinted
python3 scripts/benchmarks/summarize-decoder.py /tmp/decoder-series --output /tmp/decoder-summary
```

The current builder includes correlated tracing. To reproduce the first profile
cohort's lighter instrumentation exactly, rebuild its archived project snapshots.
Keep the tablet available for visual trials; backgrounding the replay cancels it
and aborts the controller. A completed series returns to UScreen. The retained
resume script records the original interruption/retry without overwriting its
partial result. Fixture regeneration commands and stock FFmpeg version are in each
raw archive; regeneration with another FFmpeg build may yield different bytes.

The selected outcome is the lifetime correction plus optional measured profiles,
with synchronous legacy operation still the default. Broader hardware validation,
longer power controls, transport-copy experiments and presentation-timestamp work
remain tracked by T382, T388, T403 and T414 respectively. T417 tracks a future
Android profile/capability UI.
