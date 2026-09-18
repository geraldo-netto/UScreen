# Video codecs and compatibility

The default CLI adapter supports VP9 (`libvpx-vp9`, `vp9_vaapi`) and AV1
(`libaom-av1`, `av1_nvenc`, `av1_vaapi`) through stock FFmpeg. Select the encoder in Linux settings or `config.toml`. VAAPI requires
actual encoding support in the selected GPU/driver, not just an FFmpeg wrapper
or hardware decoding. The optional in-process adapter rejects VP9 and AV1; use the
normal build. Existing configured encoder choices remain unchanged.

CLI timestamps use stock FFmpeg timing filters to keep catch-up pictures in
strict order while preserving forward wall-time gaps for sparse keyframes.
The [T421 report](reviews/2026-09-18-timestamps.md) describes the deterministic
regressions and isolated hardware checks; it does not establish an A/V latency gain.

Rawvideo startup probing is bounded to 32 bytes for every CLI codec, with the
probe picture retained (T447). Permanent stock-FFmpeg tests supply just one
picture while keeping stdin open: H.264 starts encoded output and VP9/AV1 emit
their complete framed picture. Five-picture EOF tests retain all five pictures.
An isolated H.264 VAAPI burst now retains
[all 128 supplied pictures](reviews/2026-09-18-timestamps/startup-retained.json),
compared with 127 under the earlier `nobuffer` options. T448 subsequently adds
explicit H.264/HEVC packet boundaries, so publication also completes without a
following picture or EOF. These checks do not measure physical tablet startup latency.

VP9 uses 8-bit 4:2:0 profile 0. The libvpx profile selects realtime operation,
CPU-used 8, row threading, no lookahead and no alternate-reference generation.
Its CRF quality target is combined with the configured bitrate target/maxrate;
quality numbers are not visually equivalent across codecs. VAAPI uses CQP and
is intentionally uncapped; the Linux UI disables its bitrate control and
retains that preference for other encoders. See [rate-control policy](development.md#encoder-tuning).

AV1 uses Main profile, 8-bit 4:2:0. The libaom profile selects realtime usage,
CPU-used 8, row threading, zero lookahead and no alternate-reference generation;
CRF combines with the configured bitrate target/maxrate. NVENC uses its existing
ultralow-latency policy. An FFmpeg build may list a hardware wrapper that the GPU
cannot run. Encoder support must be checked on the actual machine.

## Negotiation

New host greetings include `video_width` and `video_height`: the requested
encoded dimensions after stream scaling. Together with `fps`, these request an
Android capability report:

```json
{"type":"decoders","capabilities":{"protocol":1,"width":1280,"height":800,"fps":60,"codecs":["h264","hevc","vp9","av1"],"hardware":["h264","hevc","vp9"]}}
```

Android queries regular MediaCodec decoders for that format off the UI and
control locks. Responses from retired sockets or superseded requests are
ignored. The host requires exact dimensions/FPS and a supported protocol before
selecting VP9 or AV1. Without current evidence, it uses `libx264` while preserving the
requested encoder preference. Tablet replacement and every new authenticated controller clear capabilities,
so an older APK cannot inherit support reported by its predecessor. Capability-only updates
that leave the effective stream unchanged do not restart the encoder.

A supported-format report is advertised compatibility, not a successful decode
trial or measured performance. Encoder startup can still fail on an unsupported
host. Android selects a decoder by the actual configuration dimensions. Unknown
codec names and mismatched configuration envelopes fail closed.

T478 adds opt-in protocol version 2: new greetings include `decoder_protocol: 2`
and a `decoder_scope` string. Android echoes that scope with `protocol: 2` and
adds `details`: decoder name, codec, nullable hardware/standard-low-latency
support, nullable supported operating rate, and profile/level/depth entries.
The shared vector is [decoder-capabilities-v2.json](../testdata/decoder-capabilities-v2.json).
Each entry describes the report's exact dimensions and FPS; supported rate is
not measured throughput. API 27 leaves hardware identity unknown, and API <30
leaves standard low latency unknown. Unknown profiles/levels are omitted rather
than guessed. Reports are bounded to four families, 16 decoders, 32 profile
entries per decoder and 128 ASCII bytes per name, inside the 64 KiB message limit.

A new tablet keeps version 1 and omits rich fields with an old host. Version-one
peers keep the existing family policy. Version-two responses require the current
scope; format changes advance it even when later returning to the same format.
Malformed, unsupported-version or stale reports cannot replace current evidence.

For automatic selection with version 2, the host inspects a bounded keyframe
from the actual stock encoder probe using `ffprobe`, then intersects its codec,
profile, standard level and pixel depth with one advertised decoder. HEVC is
limited to main tier, checked in the probe SPS; unknown/high tier is rejected.
VP9/AV1 remain eight-bit 4:2:0, and only the existing HEVC conversion path can
request ten bits. This cannot create HDR or recover precision from eight-bit
capture. Missing `ffprobe`, unknown output metadata or an unsupported intersection
rejects the richer candidate and retains fallback; it never invents a level.
In particular, FFmpeg builds that omit VP9 level metadata cannot certify that
candidate for rich automatic selection. Explicit saved choices keep their policy.

The selected request appears as `decoder_selection` in the control reply and
names a decoder, stream profile and supported standard hints. Android revalidates
that exact decoder/format before allocation. Rich selections use only advertised
standard low-latency/operating-rate hints and never forward arbitrary vendor keys;
the watchdog may omit hints on recovery. Legacy/manual paths retain their
compatibility profile. A null selection clears the request. Identity or hint
changes retire the decoder generation even when codec and dimensions are equal.
Neither the request nor the probe metadata claims the live decoder honored a
hint; fresh render acknowledgements remain required. This does not alter saved
encoder choices, brightness or display refresh defaults.

The [negotiation research](media-negotiation.md) records evidence boundaries and
the separate T479 measurement/ranking work. The schema lives in the shared Rust
crate without Linux dependencies; Windows still needs its planned host adapter.

## Automatic selection

New configurations use `encoder = "auto"`; saved explicit choices are preserved.
Linux settings also expose Automatic. The control reply contains
`requested_encoder`, `effective_encoder` and `selection_reason`; the configured
preference remains `auto` when the effective encoder changes.

Until the current peer advertises support for the requested encoded geometry
and FPS, automatic mode uses `libx264`. Every controller claim and retirement
invalidates capability and selection state, including a coalesced reconnect.
A peer without the new negotiation remains on H.264. Android reports hardware
classification only when API 29+ provides it; older peers and versions remain
unknown. [Hardware classification](https://developer.android.com/reference/android/media/MediaCodecInfo#isHardwareAccelerated())
and [format support](https://developer.android.com/reference/android/media/MediaCodecInfo.CodecCapabilities#isFormatSupported(android.media.MediaFormat))
are advertised properties, not speed measurements. Decoder allocation for every supported codec uses the same format-compatible
inventory as capability reporting and prefers hardware when that classification
is available. API 27 retains platform order among compatible decoders because
hardware classification is unknown. A missing compatible decoder or a creation
failure is explicit; native creation errors identify the selected decoder and
enter the existing retirement/retry path rather than silently opening a default
codec that may reject the format.

For each advertised codec, a host worker tries its registered encoders using
the production CLI quality/options and current geometry, render node and FPS.
Each isolated process receives 65 deterministic NV12 frames with mixed spatial
detail and a moving stripe. Rawvideo probe size is bounded to 32 bytes, and the
first eight completed packets are excluded from cadence statistics. The worker
records first output, subsequent packet-interval p95 and achieved packet rate.
A missing encoder, unusable GPU, invalid output, excessive loss or eight-second
deadline rejects that candidate. Probes are serialized across tablet sessions;
they create no EVDI display or capture FIFO. The current stream continues during
these offline probes, although resource contention can affect performance.

Candidates that reach the requested FPS rank before those that do not. Within
that class, an advertised hardware decoder ranks before software/unknown, then
lower packet-interval p95 and first-output time break ties. This is a host
throughput/cadence heuristic: batching affects packet intervals, the synthetic
workload is not every desktop, and first output is not production startup time.
It does **not** benchmark Android decoder speed, sustained thermal performance,
image quality, or capture-to-display latency. Quantizer values are not equivalent
across codecs. Automatic mode attempts a performant compatible choice; it does
not claim a universally fastest codec or infer a UI gain from a newer format.

The selected candidate then needs three fresh render acknowledgements within
six seconds. Matching encoder identity and actual dimensions/rate/quality bind
that evidence to the trial; old or duplicate acknowledgements cannot certify a
replacement. Failed trials advance through the ranked list once, then restore
the prior verified selection or H.264 fallback. Encoder changes can briefly
interrupt video while preserving the capture display.

After startup verification, the selector continues observing encoder-generation
output and render progress through notifications. Three or more produced packets
without render progress for six seconds, or an encoder exit without recovery for
six seconds, trigger fallback and a two-second recovery backoff before trying the
remaining ranked candidates. Output remains observable while a failing decoder
has no video socket. A recovered render acknowledgement clears the stall window;
late acknowledgements from retired encoders cannot clear a replacement's window.
Idle content alone is not a failure. These thresholds are a bounded recovery
policy, not a latency or throughput benchmark.

Each candidate is attempted at most once in the current selection cycle. Exhaustion
keeps H.264 without claiming it has rendered successfully, and does not repeatedly
cycle through known failures. A settings/peer change permits fresh calibration;
backgrounding and shutdown cancel monitoring and pending recovery. Explicit
encoder choices retain their existing behavior.

Settings changes, controller replacement, inactive display and shutdown cancel
selection and retire its child process. Interrupted trials cannot become a
rollback target. Results stay in the current process/peer/settings epoch; there
is no disk cache, no reuse across reconnects, and no migration of saved explicit
preferences. The GPU path and depth are fixed for a daemon session; restarting
for driver/configuration changes recalibrates. The optional in-process encoder
build currently maps `auto` to `libx264`; these CLI measurements do not claim to
rank a different adapter.

## Framing

For H.264/HEVC, stock FFmpeg's `tee` muxer writes a `framecrc` metadata line and
then the unchanged encoded `data` packet to the same stdout pipe, synchronously
in that order with `flush_packets=1` on both outputs. No FFmpeg patch, secondary
metadata pipe or `use_fifo` worker is involved. The host validates bounded
headers, codec, dimensions, strictly increasing DTS, packet length and the
zero-seeded Adler-32 checksum before assembling that complete Annex B packet.
Despite the muxer's name, the checksum is Adler-32; it detects framing/content
errors and is not cryptographic authentication. Metadata flags do not establish
random access: the NAL parser retains that responsibility. One read operation
publishes at most one picture. Sparse input therefore does not wait for a
following start code or EOF. See the [T448 measurements](benchmarks/2026-09-18-packet-framing.md).

For VP9/AV1, the host consumes stock FFmpeg's
[IVF output](https://ffmpeg.org/doxygen/7.0/ivfenc_8c_source.html): a 32-byte
header followed by a 12-byte length/timestamp header per encoded packet. It
checks the version, FourCC, dimensions and frame-size bound before allocating.
Payloads retain their packet boundaries. AV1 may repeat a cached sequence header
at a join point as described below; other bytes are unchanged. IVF
container timestamps are not forwarded as a shared audio/video clock: the
existing UScreen sequence ID continues to identify render acknowledgements.

UScreen's outer TCP length/type/sequence framing is unchanged. For VP9 and AV1, type 0
contains this versioned configuration envelope; type 1 contains the encoded
packet after the sequence number:

| Offset | Bytes | Meaning |
|---|---|---|
| 0 | 4 | ASCII `USC1` |
| 4 | 1 | Codec ID: 3 for VP9, 4 for AV1 |
| 5 | 4 | Width, unsigned big-endian |
| 9 | 4 | Height, unsigned big-endian |
| 13 | remaining | Optional codec-private bytes; empty for these IVF paths |

Android uses the envelope to configure the Surface decoder; it does not queue
the envelope as compressed video. Optional private data goes into `csd-0`;
[Android documents VP9 private data as optional](https://developer.android.com/reference/android/media/MediaCodec).
AV1 receives its sequence header in-band with the keyframe; raw sequence OBUs
are not incorrectly submitted as an AV1 configuration record.
H.264 and HEVC retain their existing Annex B configuration payloads.

The VP9 header marker/profile, show-existing-frame flag, frame type and keyframe
sync code identify safe join points. Displayed keyframes permit queue recovery;
inter frames and show-existing-frame packets cannot restart a decoder. Frames
remain subject to generation retirement and the existing encoded-storage budget.

AV1 follows the [AOM low-overhead OBU syntax](https://github.com/AOMediaCodec/av1-spec/blob/master/06.bitstream.syntax.md).
The packetizer bounds OBU lengths/counts and cached sequence headers, requires
Main profile and a single primary frame per temporal unit, and rejects layered
streams. It examines the frame-header prefix to identify displayed keyframes;
inter, hidden and show-existing frames are not join points. A later keyframe
without an in-band sequence header receives the exact cached header after its
temporal delimiter, allowing a new decoder to start there. Metadata-only packets
update this cache without inventing a render sequence. The packetizer validates
framing and random-access prefixes; the decoder validates the remaining syntax.
These constraints match the supported low-latency profiles, not arbitrary AV1
files with reordering, scalability or multiple hidden frames per temporal unit.

## Validation and measurement limits

T432's normal automated tests cover fragmented input, truncation, oversized
packets, codec mismatch, metadata, keyframes, encoder retirement, old peers,
stale capability responses and changed FPS. A stock libvpx encode→packetize→decode
round trip verifies all six frames and periodic keyframes. The sparse-input test
uses the production CLI options and holds stdin open: after startup each frame
must arrive without another input frame or EOF.

During implementation, stock FFmpeg 6.1.1 live WebM with `cluster_time_limit=0`
and `flush_packets=1` emitted only headers after the first 64×64 frame; its first
cluster arrived after the second input. Subsequent clusters likewise lagged by
one input. FFmpeg's [Matroska muxer](https://ffmpeg.org/doxygen/7.0/matroskaenc_8c_source.html)
closes the previous cluster when processing the next packet. IVF avoids this
container-induced wait. This is an isolated framing observation, not an
end-to-end display-latency benchmark.

T433 adds AV1 malformed-OBU, sequence-change, profile, old-peer, stale-rate,
configuration/lifecycle and stock-CLI sparse-frame coverage. A six-frame stock
libaom encode→packetize→decode test compares decoded frame hashes with the original
IVF and independently decodes the three-frame suffix at a later keyframe. Android
API 27/34 fixtures cover codec identity, absent support and encoded dimensions;
they do not establish that a physical device implements an AV1 decoder.

The earlier [codec comparison](benchmarks/2026-09-18-codecs.md) remains historical
encoder/decoder evidence. Production VP9/AV1 support does not establish a new UI
latency, battery or quality improvement without matched live measurements.

## Doctor inventory

`uscreen doctor` treats `auto` as a selection policy and checks the required
`libx264` fallback separately from encoder wrappers. It reports the configured
preference alongside an encoder observed on the selected session's owned FIFO,
when a single recognized FFmpeg child can be identified. An absent, ambiguous
or in-process encoder remains unknown; diagnostics do not claim that a running
process has produced successful decoded frames. Doctor never claims the tablet's
control socket to inspect the selection.

The shell-protected Android inventory receiver accepts the integer extra
`uscreen_codecs_version=2`. Its response is a bounded-family text inventory:

```text
USCREEN_CODECS_V2:h264=hw;hevc=hw10;vp9=hw;av1=sw
```

Each field can contain comma-separated implementations. `none` means none was
listed, `unknown` means the query failed, and `unclassified` means a decoder was
listed without a known acceleration class. HEVC retains its `hw8`/`hw10`, `sw8`/
`sw10`, `unknown8`/`unknown10` profile entries. This is general inventory, not an
exact-resolution decoder trial. Unsupported optional codecs do not fail automatic
mode; missing required H.264 support does. Without the extra, the receiver keeps
its HEVC-only `USCREEN_CODECS_V1` response for older hosts. New hosts never use a
legacy HEVC result to infer VP9, AV1 or H.264 support.
