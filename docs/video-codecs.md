# Video codecs and compatibility

The default CLI adapter supports VP9 through stock FFmpeg `libvpx-vp9` and
`vp9_vaapi`. Select the encoder in Linux settings or `config.toml`. VAAPI requires
actual encoding support in the selected GPU/driver, not just an FFmpeg wrapper
or hardware VP9 decoding. The optional in-process adapter rejects VP9; use the
normal build. Existing configured encoder choices remain unchanged.

VP9 uses 8-bit 4:2:0 profile 0. The libvpx profile selects realtime operation,
CPU-used 8, row threading, no lookahead and no alternate-reference generation.
Its CRF quality target is combined with the configured bitrate target/maxrate;
quality numbers are not visually equivalent across codecs. VAAPI uses CQP and
retains the known uncapped-bitrate limitation described in T259.

## Negotiation

New host greetings include `video_width` and `video_height`: the requested
encoded dimensions after stream scaling. Together with `fps`, these request an
Android capability report:

```json
{"type":"decoders","capabilities":{"protocol":1,"width":1280,"height":800,"fps":60,"codecs":["h264","hevc","vp9"]}}
```

Android queries regular MediaCodec decoders for that format off the UI and
control locks. Responses from retired sockets or superseded requests are
ignored. The host requires exact dimensions/FPS and protocol version 1 before
selecting VP9. Without current evidence, it uses `libx264` while preserving the
requested encoder preference. Tablet replacement and every new authenticated controller clear capabilities,
so an older APK cannot inherit support reported by its predecessor. Capability-only updates
that leave the effective stream unchanged do not restart the encoder.

A supported-format report is advertised compatibility, not a successful decode
trial or measured performance. Encoder startup can still fail on an unsupported
host. Android selects a decoder by the actual configuration dimensions. Unknown
codec names and mismatched configuration envelopes fail closed.

## Framing

The host consumes stock FFmpeg's
[IVF output](https://ffmpeg.org/doxygen/7.0/ivfenc_8c_source.html): a 32-byte
header followed by a 12-byte length/timestamp header per encoded packet. It
checks the version, FourCC, dimensions and frame-size bound before allocating.
Payload bytes retain their original packet boundaries and contents. IVF
container timestamps are not forwarded as a shared audio/video clock: the
existing UScreen sequence ID continues to identify render acknowledgements.

UScreen's outer TCP length/type/sequence framing is unchanged. For VP9, type 0
contains this versioned configuration envelope; type 1 contains the encoded
VP9 packet after the sequence number:

| Offset | Bytes | Meaning |
|---|---|---|
| 0 | 4 | ASCII `USC1` |
| 4 | 1 | Codec ID: 3 for VP9 |
| 5 | 4 | Width, unsigned big-endian |
| 9 | 4 | Height, unsigned big-endian |
| 13 | remaining | Optional codec-private bytes; empty for the IVF VP9 path |

Android uses the envelope to configure the Surface decoder; it does not queue
the envelope as compressed video. Optional private data goes into `csd-0`;
[Android documents VP9 private data as optional](https://developer.android.com/reference/android/media/MediaCodec).
H.264 and HEVC retain their existing Annex B configuration payloads.

The VP9 header marker/profile, show-existing-frame flag, frame type and keyframe
sync code identify safe join points. Displayed keyframes permit queue recovery;
inter frames and show-existing-frame packets cannot restart a decoder. Frames
remain subject to generation retirement and the existing encoded-storage budget.

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

The earlier [codec comparison](benchmarks/2026-09-18-codecs.md) remains historical
encoder/decoder evidence. Production VP9 support does not establish a new UI
latency, battery or quality improvement without matched live measurements.
