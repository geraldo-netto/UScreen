# Measured profile selection (T479)

T479 replaces the first renderable rich candidate with a bounded comparison of
compatible encoder/decoder requests. T477 documented the evidence contract,
T478 supplied the profile/level/depth/hint intersection, and T484 tied render
ACKs to the configuration actually published by Android. Explicit encoder
preferences, brightness, refresh rate and the opt-in battery policy remain intact.

On this host/tablet, **VAAPI H.264 Constrained Baseline is the best tested
latency/fidelity tradeoff**. It substantially reduces the High-profile delay;
software libx264 is similarly fast but loses more image detail with these
production settings. Decoder names and standard hints show no clear additional
gain. This is neither a universal codec ranking nor a battery-saving claim.

## Implemented decision

[The running policy](../video-codecs.md#automatic-selection) specifies every
budget and threshold. Its dependencies are deliberately separate:

1. Scope the advertised decoder inventory to the current peer/format; inspect
   stock encoder output to intersect codec, profile, level and depth.
2. Probe host capacity and decode a deterministic first frame for a bounded PSNR
   screen. Require target FPS and quality within 0.5 dB of the libx264 reference.
3. Compare at most four encoder candidates and two decoder/hint alternatives.
   Collect fresh, receipt-matched ACKs from one encoder generation. Require at
   least 12 post-warmup samples spanning two seconds and 90% delivery; reject
   insufficient activity rather than inventing a score.
4. Prefer a p95 gain exceeding both 2 ms and 10%, with comparable ACK rate and
   bounded p99/startup regression. Within a 2 ms p95 tie, prefer at least a 1 dB
   first-frame fidelity gain under those same guards. Reverify activation, then
   monitor progress and advance through remaining successful combinations on
   failure, without cycling through failed choices.

The live measurement is **packet-ready→render-ACK**. It omits capture/encoding
and includes callback scheduling and the return route. The host probe is a
capacity/fidelity screen, not an encoding-latency percentile to add to it.
Comparisons observe current content; they do not inject a benchmark into a
user's desktop. Similar delivery rates do not establish identical content.
The quality screen uses one synthetic frame, not a perceptual desktop model.

Calibration has separate three-second admission, 30-second host-probe and
36-second live-comparison limits. Tablet input cancels measurement; an initial
quiet wait lasts at most ten seconds. Existing display/peer/settings/shutdown
guards cancel stale work. The tablet-input guard does not observe Linux
keyboard/mouse idle time. Results remain in the current process/session; no
cross-tablet cache or multi-tablet campaign was added. Version 1 peers retain
the previous capacity/rendition checks. The optional in-process adapter still
uses its documented libx264 automatic fallback.

## Physical method and scope

Device: Ulefone RugKing Pad 2 Pro, Android 16/API 36, 1280×800 stream, nominal
60 FPS, app-only 50% brightness and 60 Hz display request. Host: the existing
Ryzen 9 7945HX/RX 6600 XT setup, `/dev/dri/renderD128`, stock FFmpeg 6.1.1.
The exact fingerprint, FFmpeg build, commands, generated APK source/hashes,
input hashes and observations are in the [artifact directory](2026-09-18-profile-selection/).
No FFmpeg patch was used.

- **48 decoder replays:** High/CABAC versus Constrained Baseline/CAVLC;
  `c2.unisoc.avc.decoder` versus `OMX.sprd.h264.decoder`; supported 120 FPS
  operating-rate request versus no hints. Both hardware names advertise no
  standard low-latency feature. Each combination ran motion at 60 FPS, text at
  5 FPS and text at 60 FPS, twice with reversed order. Six measured seconds
  followed two warmup seconds. The fixtures are the previously validated,
  matched [H.264 profile corpus](2026-09-18-codecs.md).
- **24 combined USB trials / 48 phases:** current shared production encoder
  options for `h264_vaapi`, `h264_vaapi_baseline` and `libx264`, quality 18,
  configured bitrate 20,000 kbps, c2 Unisoc decoder, supported operating-rate
  request. Motion, text and pen at 60 FPS plus text at 5 FPS, two reversed-order
  repetitions. Every trial ran an initial four-second phase and a second phase
  after controlled decoder retirement/recreation. The first second of each
  phase was excluded from steady timing.
- **48 software quality decodes:** each combined phase's actual encoded output
  was decoded with stock FFmpeg, rejecting error-level diagnostics or wrong
  frame count. Compare against the exact original NV12 pictures. Actual outputs
  were eight-bit H.264 level 4.0, High or Constrained Baseline as requested.

The combined route timestamps the beginning of a paced, memory-mapped NV12
write on the host. Stock FFmpeg encodes it; checked framecrc/data tee packets
travel through an isolated ADB reverse TCP route. The copied production
`DecoderSession` sends a configuration receipt and sequence ACKs back to the
host. Host timestamps join each input frame with its own ACK. The harness has
its own transport adapter and does **not** measure the production stream-server
queue, EVDI/compositor capture, physical pixels, Wi-Fi or Bluetooth/audio sync.
No percentiles from independent clocks are summed. The same separate replay
package is used for both sides of each comparison, not the installed UScreen APK.

The harness creates no EVDI display and does not reload the daemon. The existing
helper PID remained unchanged. The installed apps and explicit `h264_vaapi`
preference were preserved; these trials do not claim the new automatic policy
has already been deployed to the user's desktop session.

## Combined raw-write→render-ACK results

Milliseconds below are the median of four per-phase percentiles: two trials,
each with initial and recreated decoder phases. Sparse phases have only 14–15
post-warmup ACKs, so their tail estimates are coarse.

| Workload | Encoder | p50 | p95 | p99 |
| --- | --- | ---: | ---: | ---: |
| Motion, 60 FPS | VAAPI High | 35.74 | 38.91 | 41.80 |
| Motion, 60 FPS | VAAPI Constrained Baseline | 19.04 | 22.25 | 23.87 |
| Motion, 60 FPS | libx264 | 18.98 | 22.88 | 25.29 |
| Text, 60 FPS | VAAPI High | 36.70 | 39.77 | 40.87 |
| Text, 60 FPS | VAAPI Constrained Baseline | 19.66 | 23.05 | 25.58 |
| Text, 60 FPS | libx264 | 17.93 | 21.49 | 25.91 |
| Pen, 60 FPS | VAAPI High | 34.49 | 37.92 | 40.72 |
| Pen, 60 FPS | VAAPI Constrained Baseline | 19.60 | 22.32 | 24.72 |
| Pen, 60 FPS | libx264 | 18.53 | 22.08 | 24.98 |
| Text, 5 FPS | VAAPI High | 222.39 | 226.00 | 227.86 |
| Text, 5 FPS | VAAPI Constrained Baseline | 23.05 | 26.47 | 28.40 |
| Text, 5 FPS | libx264 | 22.95 | 26.38 | 29.42 |

Constrained Baseline reduced motion median delay by about 47% and sparse-text
median delay by about 90% relative to High in this isolated route. The shape
matches the earlier decoder-only finding: about one input interval of added
High-profile delay. This does not isolate the vendor's internal buffering cause.

Both Constrained Baseline and libx264 returned all 480 ACKs per 60 FPS trial and
all 40 per sparse trial. High returned 478 and 38 respectively: one final picture
per phase remained unacknowledged during the bounded drain. These are achieved
short-run delivery counts; no claim of sustained multi-hour throughput follows.

Fresh decoder setup medians were 222–230 ms across the three encoders; after
controlled retirement, 52–55 ms. All 48 phase setups produced the expected
receipt. First-input→first-ACK medians across the workloads were approximately
145/136 ms for High (initial/recreated), 131/138 ms for Constrained Baseline and
95/96 ms for libx264. These startup observations include cold encoder startup
after configuration; they are distinct from steady timing and from automatic
failure-recovery deadlines. The experiment recreated a decoder deliberately;
it did not reproduce T429's original searching/reconnect symptom.

## Fidelity and bandwidth

Mean whole-frame PSNR over the four decoded phases per workload:

| Workload | VAAPI High | VAAPI Constrained Baseline | libx264 |
| --- | ---: | ---: | ---: |
| Motion, 60 FPS | 47.35 dB | 47.35 dB | 44.01 dB |
| Text, 60 FPS | 53.64 dB | 53.64 dB | 44.43 dB |
| Pen, 60 FPS | 53.35 dB | 53.35 dB | 44.99 dB |
| Text, 5 FPS | 53.52 dB | 53.52 dB | 34.56 dB |

Text-crop PSNR also favored both VAAPI profiles over libx264 in each workload.
High versus Constrained Baseline differed by less than 0.003 dB here; the older
fixed-GOP profile cohort additionally retained identical decoded hashes.
These are software-decoded fidelity metrics, not pixel capture of the tablet.

Constrained Baseline used about 23.3% more motion bytes than High, versus the
earlier cohort's 23.8%. Text/pen differences were smaller. VAAPI CQP intentionally
has no bitrate ceiling; libx264 retains its CRF/VBV policy. Equal quality numbers
do not imply equal fidelity. In this cohort, libx264 also used more bytes than
Constrained Baseline. Raw byte counts and per-plane/crop results are retained.

A separate check using the deterministic first probe picture and current
exported encoder options measured 35.29 dB for libx264 and 58.93 dB for both
VAAPI profiles. Its reference hash and commands are retained; it reproduces the
fidelity calculation rather than the full 65-frame cadence probe. It therefore admits the
two VAAPI profiles. The full corpus supports using fidelity to break their
near-latency tie with libx264; the screen is still only a bounded heuristic for
other content or hardware. A faster candidate does not receive an unrestricted
quality exemption: all candidates must meet the baseline-relative floor.

## Decoder hints, other codecs and power

The 48 negotiated decoder replays reproduced approximately 11 ms motion
feed→release for Constrained Baseline versus 28 ms for High; sparse text was
13 versus 213 ms. Swapping c2/OMX names or disabling operating-rate hints changed
these timings by substantially less than the 2 ms switching threshold. No extra
speed or power gain is established for those variations; do not hardcode a
vendor name or infer that advertised 2× rate improves latency.

The [earlier matched codec cohort](2026-09-18-codecs.md) remains the VP9/AV1
comparison evidence; this batch does not relabel it as a new protocol-2 run.
T478 currently rejects rich VP9 candidates when stock ffprobe supplies no known
level. Exact-format Main10 support is absent from this tablet's current rich
inventory. VAAPI HEVC's advertised support does not repair the independently
reproduced T422 native failure. Runtime trials still require fresh real ACKs.
AV1's software decoder advertises standard low latency, but its earlier measured
motion delay was much larger. A different current runtime measurement can win
the bounded comparison; codec family and hardware labels alone cannot.

Battery service snapshots reported USB power with a 500 mA / 5 V charging limit.
The 48 replay cohort's charge-counter range was 4,925,070–4,955,040 µAh; the
combined cohort's was 4,885,110–4,905,090 µAh. Those short trials span coarse
9.99 mAh steps and Activity transitions. Battery temperatures ranged 30.8–31.3°C.
Global thermal status was zero in the inspected final snapshot, while individual
SoC/GPU sensors reported status one: this is not proof of no thermal limitation.
No sustained balanced per-profile energy winner is established. Power remains
unknown in ranking, and T388's sustained validation remains blocked separately.

## Validation and reproduction

Permanent normal-suite tests cover ranking versus the old first-result behavior,
quality ties, unknown fidelity, capacity/loss rejection, sparse samples, jitter,
different delivery rates, bounded observation windows, stale generations,
controlled decoder recreation and user-input cancellation. Red logs retain the
initial ranking/quality failures and the replay's rejected intentional reset;
the same tests pass with the final implementation. Existing manual-choice,
peer/format/background cancellation and later-failure recovery regressions pass.

The full Rust workspace passes 358 host, 57 shared and 51 GUI tests (three
existing host tests ignored). Android passes 376 tests, lint and debug APK build.
The normal Cargo tooling test discovers the Python benchmark regressions.
Default and optional in-process Clippy pass with warnings denied; project
cyclomatic complexity remains at most nine.

`decoder-project.py` builds a separate replay APK with exact copied production
decoder sources. `decoder-plan.py` runs the archived negotiated plans.
`profile-usb.py` consumes the same verified NV12 corpus, exports current options
through `common/examples/encoder-options.rs`, creates one non-rebinding loopback
ADB reverse route and removes it afterward. Focus/input loss stops the current
new replay APK. T485 removes host-side relaunches after successful runs too:
Android finishes the benchmark Activity's own task, preserving another app
selected by the user during completion. No installed user
preferences are changed. The initial USB smoke exposed an intentional-reset
classification error in the new harness; it was excluded, fixed and covered by
an API 27/34 regression before the successful 24-trial cohort.

Recompute timing summaries from extracted artifact directories:

```bash
python3 scripts/benchmarks/summarize-profile-selection.py \
  --decoder /path/to/decoder-raw --usb /path/to/usb-raw \
  --output /tmp/profile-selection-summary.json
```

The preserved APK source snapshots identify the actual measured code. Historical
raw source snapshots stay immutable even when the current harness subsequently
adds better failure-observation retention. The full-path evidence still missing
is compositor/EVDI-to-physical-pixel timing, sustained balanced energy/thermal
validation and reproduction of the original reconnect symptom. None is claimed
as solved by this latency/fidelity selection policy.
