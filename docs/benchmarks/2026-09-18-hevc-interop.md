# T422 — AMD VAAPI HEVC / Unisoc interoperability

The affected tablet still rejects the tested AMD VAAPI HEVC streams with native
MediaCodec error 14. No working stock encoder option or Android configuration
was found. T422 remains blocked on a compatible stock configuration or a
validated encoder/driver/firmware correction; no FFmpeg patch, undocumented
bitstream rewrite or production codec-policy change is included.

## Controlled comparison

Host: RX 6600 XT / Navi 23, Mesa 26.2.2, stock Ubuntu FFmpeg 6.1.1. Tablet:
RugKing Pad 2 Pro, Android 16, `c2.unisoc.hevc.decoder`. The source is T400's
four-second 1280×800/60 motion corpus. Original VAAPI uses Main profile, 8-bit
NV12, CQP 21, no B pictures and a 60-frame GOP. The x265 positive control is the
previously preserved T400 selection. These are compatibility trials, not a new
quality/latency ranking.

The replay copies production decoder sources and observes their codec factory.
T458 repaired missing capability-source/dependency wiring and the changed
factory signature. T460 aligned the replay's display-over-keyguard flags with
UScreen. Before T460, secure keyguard paused the replay and retired its Surface;
those interrupted trials were discarded. The corrected x265 control rendered
240/240 measured frames at an observed 60 Hz while the keyguard remained secure.

Each accepted trial used one second of warm-up and four measured seconds at
60 encoded frames/s. A decoder invalidation is a failure, not successful support.
The same replay, Surface path and stream boundaries were used for each paired
configuration experiment.

| Configuration delivery / decoder policy | Original VAAPI | Original x265 |
| --- | --- | --- |
| Separate codec-config buffer; legacy hints | Error 14; 0 measured frames | 240/240 rendered |
| Callback decoder; no latency/operating-rate hints | Error 14; 0 | 240/240 |
| Parameter sets in `csd-0` at configure; no duplicate config buffer | Error 14; 0 | 240/240 |
| Parameter sets only in first encoded access unit | Error 14; 0 | 240/240 |

Both original parameter-set buffers already use four-byte Annex-B prefixes.
Changing their delivery did not remove the failure. Removing optional AUD and
SEI units with FFmpeg's documented `filter_units` bitstream filter also failed.

## Stock encoder controls

All ten VAAPI variants below failed before measured output. Options were
changed separately from the original command unless stated otherwise; command
arrays, logs, fixture bytes and hashes are retained.

- Asynchronous depth 1.
- One reference or four references.
- All-intra encoding (`-g 1`).
- Explicit levels 4.1 or 5.1.
- CBR or VBR at 20 Mb/s with a 20 Mb buffer.
- 1280×768 crop or 1280×832 padding, to test CTU-aligned heights.

The low-power entry point was unavailable. A four-slice attempt produced no
usable frame packets; it was rejected before Android replay. Neither case is a
successful compatibility workaround.

Seven stock x265 feature controls all rendered 240/240 measured frames: CTU 64 /
minimum CU 8, transform hierarchy depth 4, transform skip, AMP plus SAO, disabled
temporal MVP plus strong intra smoothing, disabled wavefront parallelism, and a
combined version of those settings. Thus those selected feature differences,
individually or combined in x265 output, did not reproduce the VAAPI failure.
They do not make the two encoders' bitstreams structurally identical.

All twenty preserved full fixtures produce 240 frames with stock FFmpeg's
software decoder and return code zero. **Sixteen have no decode errors.** CBR,
VBR, all-intra and the 768-pixel-height variant emit CU-QP/CABAC decoding errors
despite returning frames; those four are not clean interoperability controls.
The original VAAPI/x265 fixtures and the other controls decode without errors.
Frame count and process success alone therefore cannot establish valid decoding
(T461). Even clean software decoding is not a proof of complete HEVC conformance
or proof that the tablet firmware is solely responsible.

The 26 device replays comprise 15 failed VAAPI cases and 11 successful
x265 cases. Excluding the four variants with software decode errors leaves
22 clean-software comparisons: 11 native failures and 11 native successes. Successful trials observed 60 Hz. Two very short failed variants
reported 90 Hz before their window refresh request settled; no latency or
refresh-dependent performance claim is made from these failures. Native error
14 also occurs in the otherwise matched 60-Hz original/configuration trials.

## Interpretation and fallback

The evidence narrows the problem to compatibility between these VAAPI-generated
streams and this hardware decoder, beyond the tested framing/CSD/hint choices.
It does not identify a single failing syntax field or establish which component
violates a contract. A symbolized vendor decode trace or a stock configuration
that changes the outcome is still needed before a cause-specific fix.

UScreen's existing automatic selector still requires real render ACKs before
accepting a candidate and retains its H.264 fallback when candidates fail;
`host/src/selection/worker.rs` and its normal regression suite enforce that
contract. Advertised HEVC capability is not confirmation of decoding. The saved
explicit H.264 VAAPI setting was preserved, and the installed app returned to its
working stream. No live VAAPI→HEVC migration or fallback timing improvement is
claimed by this isolated replay.

## Reproduction and artifacts

The [artifact directory](2026-09-18-hevc-interop/) contains:

- `fixtures.tar.xz`, all twenty complete `USDB0002` replay fixtures;
  `fixtures.json`, hashes, configuration bytes, dimensions and software decode
  counts/checksums.
- `trials.tar.gz`, raw Android logs/results, frame traces, exact fixture hashes,
  APK identity and host timestamps; `summary.json`, per-case outcomes.
- `encoder-commands.tar.gz`, stock encoder commands and logs, including rejected
  controls; `replay-sources.tar.gz`, the exact isolated replay sources/manifest.
- Experiment scripts and `SHA256SUMS`. The scripts retain recorded local paths
  and tablet serial; adjust these for a new machine. The original generated
  corpus and method are documented in [T400](2026-09-18-codecs.md).

For the unmodified production-source replay, generate an APK with
`scripts/benchmarks/decoder-project.py`, install that separate APK, extract the
fixtures, and run `decoder-plan.py` with profile `legacy`, rate 60, seconds 4,
warm-up 1 and the generated APK provenance. Run `original-vaapi.bin` and
`original-x265.bin` in separate plans: the normal runner intentionally aborts
on the failed VAAPI result. The archived experimental sources additionally
provide `csd-format` and `inband-only` controls.

A future compatibility fix must retain a permanent failing regression for the
identified cause and verify the same native fixture on this tablet. A fake
MediaCodec test cannot establish that vendor-native error 14 is corrected.
