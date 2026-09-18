# H.264/HEVC packet framing (T448)

Stock FFmpeg packet framing removes one source-frame interval from the host's
input-to-packet-ready path in this experiment. At 640×400/60 FPS, median time
falls from 17–19 ms to 0.75–2.62 ms across the tested encoders and one/two/four
streams. At sparse 5 FPS it falls from about 201–205 ms to 0.85–5.04 ms.
No FFmpeg source changes or wire-protocol changes are involved.

These are synthetic host measurements, **not capture-to-display or Android
measurements**. No tablet was attached. The earlier device baseline starts its
main timer after this packet-assembly stage, so its reported latency omits the
wait removed here. Do not subtract or add these independent percentiles to
estimate a physical display result.

## Implementation

Previously, the last Annex B NAL and access unit waited for a later picture
delimiter or EOF. Stock FFmpeg's [tee muxer](https://github.com/FFmpeg/FFmpeg/blob/n6.1/libavformat/tee.c)
can instead write two synchronous outputs to the same pipe, in order:

```text
-map 0:v:0 -f tee '[f=framecrc:flush_packets=1]pipe:1|[f=data:flush_packets=1]pipe:1'
```

[framecrc](https://github.com/FFmpeg/FFmpeg/blob/n6.1/libavformat/framecrcenc.c)
supplies packet size and a zero-seeded Adler-32 checksum before
[data](https://github.com/FFmpeg/FFmpeg/blob/n6.1/libavformat/rawenc.c) writes the
unmodified encoded bytes. The data muxer avoids automatic H.264/HEVC bitstream
filters that could invalidate that size/checksum. Both outputs flush every
packet; `use_fifo` stays disabled. This avoids a second descriptor, cross-pipe
coordination and a custom NUT demuxer. NUT was not benchmarked or implemented;
the selected tee path directly preserved the packet boundary needed here.

The host limits header lines to 512 bytes and the initial header to 32 lines,
validates the video stream/codec/dimensions/time base, requires increasing DTS,
and bounds payload allocation to the existing 8 MiB-minus-sequence limit.
It checks the checksum before parsing NALs, retains configuration and
metadata-only prefixes, and emits at most one picture per encoded packet.
Existing CSD, keyframe, queue-budget, sequence and generation rules remain.
The checksum detects framing/content errors; it is not authentication.
VP9/AV1 keep their IVF framing.

## Method and results

The permanent opt-in `capture/framing_profile.rs` replay uses production CLI
arguments, stock FFmpeg 6.1.1-3ubuntu5 and the RX 6600 XT render node. The legacy
comparison changes only output framing and uses the retained incremental
parser. Both retain the first raw picture and T421 timestamp correction.
VAAPI uses the production capability probe and `async_depth=1` on this host.
The deterministic NV12 checker/stripe workload supplies 25 pictures at 5 FPS
or 120 pictures at 60 FPS, with three repetitions and alternating order.
All supplied pictures must be received. Latency starts immediately before the
input write, includes pipe admission/encoding/assembly, and ends at publication
readiness on the same host monotonic clock. Statistics omit the first eight
pictures and the final EOF-flushed picture; raw samples retain them all.

Values below are pooled **p50 / p95 milliseconds**. The
[summary](2026-09-18-packet-framing/summary.json) also contains p99 and counts.

| Encoder | Input FPS | Streams | Legacy Annex B | Framed packets |
|---|---:|---:|---:|---:|
| libx264 | 5 | 1 | 200.93 / 201.54 | 0.85 / 1.07 |
| libx264 | 5 | 2 | 200.91 / 201.62 | 0.89 / 1.10 |
| libx264 | 5 | 4 | 200.87 / 201.61 | 0.94 / 1.22 |
| libx264 | 60 | 1 | 17.47 / 18.28 | 0.75 / 0.93 |
| libx264 | 60 | 2 | 17.48 / 18.46 | 0.79 / 1.00 |
| libx264 | 60 | 4 | 17.52 / 18.57 | 0.77 / 1.13 |
| h264_vaapi | 5 | 1 | 203.70 / 204.41 | 3.65 / 4.22 |
| h264_vaapi | 5 | 2 | 203.81 / 204.99 | 4.01 / 4.97 |
| h264_vaapi | 5 | 4 | 204.01 / 205.61 | 4.43 / 6.24 |
| h264_vaapi | 60 | 1 | 18.46 / 19.55 | 1.77 / 2.14 |
| h264_vaapi | 60 | 2 | 18.78 / 20.30 | 1.89 / 2.76 |
| h264_vaapi | 60 | 4 | 19.22 / 21.22 | 2.41 / 4.26 |
| hevc_vaapi | 5 | 1 | 203.88 / 205.75 | 3.76 / 5.38 |
| hevc_vaapi | 5 | 2 | 204.04 / 205.81 | 3.82 / 5.91 |
| hevc_vaapi | 5 | 4 | 204.68 / 207.57 | 5.04 / 8.05 |
| hevc_vaapi | 60 | 1 | 18.50 / 19.57 | 1.73 / 2.03 |
| hevc_vaapi | 60 | 2 | 18.66 / 20.42 | 1.85 / 2.83 |
| hevc_vaapi | 60 | 4 | 19.11 / 20.70 | 2.62 / 4.27 |

Mean process CPU seconds for the complete four-stream, 120-picture-per-stream
60 FPS trials are below. Host CPU includes the replay feeder and parser; encoder
CPU sums all FFmpeg children. These are elapsed CPU seconds, not percentages or
isolated parser costs. Small differences are within the variability of this
three-repeat run; the measurements establish no large CPU penalty.

| Encoder | Host CPU, legacy → framed | FFmpeg CPU, legacy → framed |
|---|---:|---:|
| libx264 | 0.0620 → 0.0566 | 0.8973 → 0.8687 |
| h264_vaapi | 0.0479 → 0.0478 | 0.6059 → 0.5993 |
| hevc_vaapi | 0.0538 → 0.0557 | 0.7087 → 0.7139 |

Initial VAAPI controls omitted the optional depth argument, using FFmpeg's
default depth two. H.264 then measured about 401→201 ms at 5 FPS and 34→17 ms at
60 FPS for one stream: packet framing removed the assembly interval but left
the encoder's extra interval. Those controls are preserved and explicitly
separated from the depth-one production comparison.

Raw samples: [libx264](2026-09-18-packet-framing/libx264-default-depth.json),
[H.264 VAAPI depth one](2026-09-18-packet-framing/h264_vaapi-depth1.json),
[HEVC VAAPI depth one](2026-09-18-packet-framing/hevc_vaapi-depth1.json),
[H.264 default-depth control](2026-09-18-packet-framing/h264_vaapi-default-depth.json),
[HEVC default-depth control](2026-09-18-packet-framing/hevc_vaapi-default-depth.json),
and [environment/source identity](2026-09-18-packet-framing/environment.json).
The measured implementation is base `4f2598b` plus the T448 changes committed
with this report. Source hashes distinguish the implementation and final replay
harness. CPU/GPU clocks, workload, resolution and concurrent system activity
limit generalization. NVENC and other drivers were not measured.

## Reproduction and regression coverage

```sh
USCREEN_PROFILE_ENCODER=h264_vaapi cargo test --locked -p uscreen --release \
  --bin uscreen t448_packet_framing_profile -- --ignored --nocapture
```

Use `libx264` or `hevc_vaapi` for the other measured encoders. The opt-in replay
creates only encoder children; it does not attach EVDI or touch the desktop.
The ignored test is a timing experiment. The following correctness checks run
in the normal automated suite:

- A production libx264 regression first failed by waiting for the successor
  picture. It now publishes each supplied sparse picture while stdin stays open.
- Fragmented headers/payloads, malformed metadata, codec/size/checksum mismatch,
  truncation, duplicate DTS, multiple pictures, metadata-only prefix retention,
  EOF and generation retirement are covered.
- A 1 MiB encoded packet crosses a 64-byte test pipe without a successor or EOF;
  the existing exact maximum-size and configuration/AU bounds still pass.
- Stock H.264 and HEVC encode→frame→assemble→decode tests preserve all decoded
  pictures and independently decode later keyframe suffixes.
- Existing capture cancellation, first-picture retention, wall-time keyframes,
  timestamp-order and copy-budget regressions remain in the suite.

Recommendation: use the framed CLI path. Keep latency claims at this measured
boundary until the tablet is available for matched physical replay. T416
separately measures the bounded publication batch and metadata-allocation cost.
