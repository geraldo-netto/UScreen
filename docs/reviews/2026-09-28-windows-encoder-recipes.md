# T685 Windows encoder probe recipes

`common::encoding::windows` defines a bounded probe catalog for H.264/HEVC
NVENC, AMF and QSV, plus libx264. It validates shared FPS, bitrate, quality,
worker and raw geometry limits before producing stock-FFmpeg arguments.
The existing production catalog, defaults and libx264-only pipe admission remain
unchanged. These recipes are preparation for T530; GPU/session enablement is T677.

libx264 and NVENC reuse existing shared profiles. AMF requests ultralowlatency,
speed, CQP, no B frames and asynchronous depth one. QSV requests veryfast, CQP,
no B frames and asynchronous depth one; only H.264 receives look_ahead=0.
HEVC Main10 requests p010le. H.264 rejects Main10; HEVC rejects H.264 profiles.
Actual codec/profile/depth/level must be inspected and matched against the tablet
advertisement before selection. A requested profile is not observed output.

Quality stays within 12–32. AMF/QSV CQP does not enforce the requested bitrate
ceiling: the bounded bitrate value is retained in the request, but no misleading
maxrate argument is emitted. Lower quantizers favor quality at higher bandwidth.
Hardware requests require workers=0, meaning native threading is unknown; CPU
worker values cannot configure GPU parallelism. libx264 zero starts with one
worker, while T530 may probe the shared automatic 1/2/4 candidates. Explicit
1–128 values use the existing worker policy. No throughput or latency benefit is
claimed from recipe syntax.

Native option inventories are retained with the test evidence. The isolated
Windows test launches each recipe against generated frames under an owned job
and deadline; unavailable GPUs stay unavailable. The CPU recipe must encode.
T530 adds bounded discovery, packet/format verification and selection diagnostics;
physical GPU/tablet acceptance is still separate.

Sources: [FFmpeg QSV rate control and options](https://ffmpeg.org/ffmpeg-codecs.html#QSV-Encoders)
and the retained FFmpeg 8.1.2 native `-h encoder=...` inventories. FFmpeg documents
that QSV may choose another mode depending on the system, so effective settings
and successful output need native evidence.
