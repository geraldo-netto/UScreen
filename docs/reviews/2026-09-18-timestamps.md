# Stock FFmpeg timestamp correction (T421)

The installed baseline journal contains H.264 muxer warnings for equal DTS
values. FFmpeg 6.1's
[wall-clock input implementation](https://github.com/FFmpeg/FFmpeg/blob/n6.1/libavformat/demux.c#L589)
rescales the clock into the input time base. Rawvideo at 60 FPS therefore has
coarse timestamp steps: pictures read during a catch-up burst can share a value.
A wall-clock adjustment can also move that value backwards.

The CLI now uses stock `settb`/`setpts` filters with a microsecond time base and
an explicit matching encoder time base. Each output timestamp is at least one
microsecond later than its predecessor. Forward wall-time gaps remain intact,
so the existing one-second forced-keyframe expression still works for sparse
input. These filters change frame timing metadata; existing NV12/10-bit
conversion and VAAPI upload remain in the same filter chain. No FFmpeg source
changes, frame dropping or duplication were introduced.

See FFmpeg's [timestamp filters](https://ffmpeg.org/ffmpeg-filters.html#setpts_002c-asetpts)
and [encoder time-base option](https://ffmpeg.org/ffmpeg.html#Advanced-options).
This does not recover capture timestamps lost before FFmpeg, create an audio
clock, or reconstruct elapsed time after a backwards system-clock step.

## Validation

The permanent `capture/timestamp_tests.rs` regression uses production output
options with a deterministic synthetic input and NUT output so timestamps can
be inspected. Forty pictures include repeated timestamps, a backwards jump and
forward gaps. The original implementation emitted non-monotonic-DTS warnings.
The corrected implementation passes for libx264, libvpx-vp9 and libaom-av1:

- All 40 pictures decode in their original order, checked with distinct flat
  luma values and a small lossy-coding tolerance.
- Packet DTS values strictly increase.
- Key pictures occur at 0, 1.1 and 2.2 seconds, rather than depending on 60
  submitted pictures for each keyframe.
- No timestamp-order warning is emitted.

An additional isolated test on the host's `/dev/dri/renderD128`, using distribution
FFmpeg 6.1.1-3ubuntu5, repeated the timestamp pattern at 256×256 with H.264 and
HEVC VAAPI. Both emitted 40 packets, decoded all 40 pictures in software, retained
the same three keyframe times and produced strictly increasing DTS without
warnings. This did not involve EVDI, the desktop compositor or Android.

A separate 128-picture NV12 catch-up burst through stock H.264 VAAPI and its raw
H.264 muxer reproduced eight equal-DTS warnings before the change and zero after
it. Both runs produced 2,322 encoded bytes and 127 decoded pictures without
software decoder errors. The initial rawvideo probe picture was consumed in
both runs; T447 tracks that separate startup issue. The temporary burst replay
bounded raw probing to 32 bytes to isolate timestamp behavior.

Recorded commands and results: [GPU pattern](2026-09-18-timestamps/gpu-pattern.json),
[raw burst](2026-09-18-timestamps/vaapi-burst.json),
[before diagnostics](2026-09-18-timestamps/vaapi-before.log) and
[after diagnostics](2026-09-18-timestamps/vaapi-after.log). Temporary encoded files
were decoded during the checks and then removed; the permanent fixture preserves
the timestamp/content pattern for independent reproduction.

This validates timestamp correctness, not a measured change in physical
capture-to-display latency, battery use or the reported Bluetooth A/V offset.
The Android replay and A/V investigation remain separate tasks.
