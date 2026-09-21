# T575: implemented X11 GPU capture prototype

**Keep FIFO as the default.** The GPU prototype lowers measured process CPU,
but source-update-to-render-ACK latency worsened in every dephased pair.
Cross-device implicit-layout imports also corrupt nonuniform images on this
machine, so the implemented adapter rejects them before emitting a stream.

The prototype, its [build/activation instructions](../../gpu-capture.md) and
permanent tests are committed. It is explicitly enabled with
`USCREEN_X11_GPU_HELPER`, not automatically included in the AppImage or enabled
on the user's live installation. Stock FFmpeg 6.1.6 and libevdi are unchanged.
T569 supplied the clean Android replay APK used here; T570 established the
shared damage histories that subsequent capture work can reuse.

## Matched physical USB experiment

Host: Cinnamon/X11, Ryzen 9 7945HX, kernel 7.0.0-31, Mesa 26.2.3. X11 renders
on Radeon 610M/Raphael (`renderD129` in this boot); the existing FIFO encoder
uses Radeon RX 6600 XT/Navi23 (`renderD128`). Tablet: Ulefone RugKing Pad 2 Pro,
`c2.unisoc.avc.decoder`, operating rate 60, low-latency flag disabled. Both
routes encode 1280×800, H.264 Constrained Baseline, QP 18, 30 FPS setting,
no B-frames, asynchronous encode depth one. This compares the existing
two-GPU arrangement with capture/encoding on the X11 GPU; it does not isolate
GPU selection from capture implementation.

An unused EVDI card/output owns a separate temporary monitor. A double-buffered
X11 scene draws a moving colored rectangle plus a 24-bit frame-identity
barcode. The live tablet display remains attached throughout. Decoding every
encoded picture identifies the actual source update; physical Android render
ACKs must cover every encoded packet exactly once. No matching by throughput
or estimated frame index.

The timed boundary is **XCopyArea update submission through receipt of Android's
render callback ACK on the host**. This includes host scheduling, capture,
conversion, encoding, USB, decoder and ACK return. It excludes application
drawing before submission and does not measure optical display latency.
Repeated encodes of an unchanged source identity count only at its first ACK.
The first 30 encoded pictures of each trial are warmup.

Primary experiment: five alternating pairs, 480 encoded pictures per trial,
**29 source updates/s versus 30 FPS capture**, reducing equal-cadence phase
bias. All **4,800/4,800** encoded pictures received ACKs.

| Pair | FIFO p50 / p95, ms | GPU p50 / p95, ms |
| --- | ---: | ---: |
| 1 | 27.71 / 38.36 | 37.03 / 54.44 |
| 2 | 28.72 / 38.66 | 38.24 / 52.89 |
| 3 | 28.83 / 38.26 | 37.12 / 52.29 |
| 4 | 28.32 / 37.56 | 37.43 / 52.86 |
| 5 | 28.81 / 38.18 | 38.62 / 54.11 |
| Median of trial percentiles | **28.72 / 38.26** | **37.43 / 52.89** |

GPU capture increases median p50 by **30.3%** and p95 by **38.3%**. Both
metrics worsen in all five pairs. After warmup, FIFO delivers 450 unique
updates/trial and the fixed-cadence GPU path 434–436; duplicated pictures do
not count as fresh updates. Coverage of source IDs within each observed range
is 100% except one GPU trial at 99.77%. The FIFO trial takes roughly 16.6 seconds
for 480 new pictures at the scene's cadence; GPU capture takes 16 seconds and
includes duplicates.

Median combined encoder/capture-helper CPU seconds per elapsed second falls
from **0.07883 to 0.05303**: **32.7% lower**, or about 7.88% to 5.30% of one
core. This includes either FFmpeg or the GPU helper, plus the retained EVDI
helper. It excludes Xorg/compositor, the scene process, Android and the other
live UScreen session. No GPU utilization, total-system energy or battery
improvement is established.

An earlier 30 Hz source/30 FPS capture run favored GPU capture in four of five
pairs but lost in the fifth. The later differing-cadence experiment reverses
that apparent advantage consistently. Both series are retained; the early
equal-cadence numbers must not be advertised as a latency improvement.

The primary five-pair checkpoint precedes the final additional post-copy
geometry validation and explicit forwarding of the maximum bitrate parameter
and CQP mode. Exact checkpoint sources/hashes are archived. Two final-code
confirmation pairs, 240 pictures per trial and 29 Hz source, give:

| Pair | FIFO p50 / p95, ms | Final GPU p50 / p95, ms |
| --- | ---: | ---: |
| 1 | 28.95 / 38.48 | 39.26 / 53.57 |
| 2 | 28.89 / 37.75 | 37.57 / 53.28 |

All **960/960** pictures received ACKs; the latency penalty persists in both
pairs. These are confirmation runs, not an additional independent speedup.

## Correctness and lifetime evidence

- Same-device patterned frames preserve the source colors within three RGB
  levels after H.264 decoding, at scales 1–4. The isolated Xvfb test verifies
  cursor hotspot/crop edges and rejects pixels outside the captured region.
- Cross-device API success was insufficient: the earlier stream had stripes
  and invalid gray barcodes despite complete ACKs. Synthetic images and
  encoded outputs are retained. The final helper checks the DRI3 producer and
  configured render descriptor identities and rejects a mismatch with zero
  output bytes. The earlier uniform-color feasibility probe missed this.
- Native move, rotation, nonidentity transform and disconnect checks all end
  capture, followed by a successful fresh adapter on the same temporary
  monitor. Native stdout backpressure triggers the process deadline. The host
  regression verifies one-way fallback, owned-child cancellation and retained
  EVDI/FIFO ownership, including reader retirement before FIFO replacement.
- XSync trigger/await establishes producer GPU completion. VAAPI conversion
  completes before releasing the single RGB lease; independently owned NV12
  frames remain with libavcodec until final release. GPU handles never enter
  the portable session API.
- Existing Xorg PID 2512 and live EVDI helper PID 1160852 remain unchanged.
  The temporary output is disconnected after each run; Android's production
  activity and ADB routes are restored. No power/lock, service restart or
  ADB-server reset is performed.

## Permanent tests and measurements

Normal host tests cover T575 adapter admission/settings, real supervisor
startup, failed retry, cancellation and one-way fallback. A six-frame native
synthetic fixture in `host/tests/fixtures/t575-gpu.tee` passes the existing Rust
framed-Annex-B parser, including CSD, IDR, frame count and monotonic timestamps.
The settings regression was demonstrated failing before bitrate forwarding,
then passing after it. Native bounds/EDID/transform tests include bounded
invalid-input sweeps under AddressSanitizer and UndefinedBehaviorSanitizer.

Before the final settings/fixture additions, the full default binary suite
passed 364 tests (three existing ignored), and the in-process configuration
passed 250 (two existing ignored). All seven final T575 binary tests and the
native C integration test then passed. Combined LLVM counters show **52/52**
maintained Rust functions across the three touched capture modules at ≥80%
line coverage. Optional native Xvfb and real-GPU suites yield **38/38** C GPU
functions at ≥80%, minimum 92.86%. This is scoped evidence, not a claim that
every repository platform or coverage gate passes.

Windows GNU cross-compilation passes. Clippy passes with the existing T573
`manual_is_multiple_of` allowance.
All 5,528 inventoried functions meet cyclomatic complexity ≤9. Six Makefile
tests and both GPU measurement/report integrity tests pass.

The [evidence archive](evidence.tar.gz) contains checkpoint and final source
copies, binary hashes, arguments, scene identities, encoded synthetic streams,
ACKs, CPU observations, rejected cross-device pictures, native lifecycle logs,
regression logs and GCC counters. Its hash is in [metadata](metadata.json).
Readable summaries: [paired timing](paired-summary.json),
[final confirmation](final-summary.json), [native coverage](native-coverage.json)
and [Rust coverage](coverage-final.json).

## Remaining work

T579 tracks event-driven GPU capture: periodic polling is a plausible source
of the added delay, not a demonstrated complete diagnosis. T580 tracks safely
stopping redundant EVDI CPU conversion while retaining monitor ownership.
T578 tracks explicit cross-device modifiers/layout validation. T577 separately
records existing input mapping's incorrect sysfs/RandR name assumption.

Full-pipeline zero-copy remains unproven. The current prototype avoids desktop
CPU readback/upload in its own GPU encoding path, but retained EVDI work, Xorg
copies, GPU conversion, cursor pixels and USB/Android buffers remain.
