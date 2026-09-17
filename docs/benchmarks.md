# Benchmarks

Historical results inherited from the [upstream project](https://github.com/majmichu1/UScreen),
retained with their original numbers. They describe one host and one tablet,
not measurements rerun on this fork. The initial series covers 1.0.0–1.1.0;
the CPU/capture sections also contain explicitly labeled 1.2.0 follow-ups.
Their separate measurement dates were not recorded here. No raw sample set
is included, so the statistical results cannot be independently reconstructed
from this document alone.

## Test configuration

| | |
| --- | --- |
| Initial series dates, as reported | 2026-08-26 to 2026-08-31 |
| Initial series versions | upstream 1.0.0–1.1.0; later comparisons labeled below |
| Host | Laptop, NVIDIA GeForce RTX 5060 Laptop GPU, Bazzite (Fedora Atomic) with KDE Plasma 6 on Wayland, kernel 7.2 |
| Tablet | Samsung Galaxy Tab S9 Ultra (Snapdragon 8 Gen 2), 2960×1848 @ 90 Hz |
| Link | USB-C cable, adb over USB; Wi-Fi tests on a 5 GHz (tablet) / 6 GHz (host) link to the same router, RSSI −37 / −44 dBm |
| Stream | 2960×1848, 90 fps target, constant-quality VBR (`quality = 12`, `bitrate` cap 60 Mbps), H.264 NVENC unless stated |
| Desktop content | a mostly static desktop with occasional window movement; bitrate on a static desktop settles around 0.5–4 Mbps |

## How latency is measured

The daemon starts a host-clock timer when a complete encoded access unit is
ready for broadcast, after encoding and CLI packetizer buffering. The app
returns its sequence number after Android's `onFrameRendered` callback runs;
the timer stops when the host receives that acknowledgement over the input
socket. The legacy log label **encode→display** therefore measures **encoded
packet ready → render acknowledgement received**, including forward queueing,
transport, decoding/rendering, callback scheduling and the return message.
It is not a camera measurement of pixels becoming visible.

The tablet's **decode+render** value covers complete-frame arrival to execution
of its render callback. The legacy **wire** estimate subtracts its median from
the host median; it includes host queueing and the reverse acknowledgement path,
and is not an isolated USB-hop measurement or an exact per-frame decomposition.

Capture, encoding, and time already spent assembling an access unit are outside
that host timer. The helper's separate `capture→fifo` timer begins after
`evdi_grab_pixels` returns and ends after the frame is written to the FIFO;
it excludes the compositor wait and the grab itself. These uncorrelated metrics
do not add up to a measured total display latency.

## Results

### Encoded packet to acknowledgement, USB

| codec | p50 | p95 | notes |
| --- | --- | --- | --- |
| H.264 (NVENC) | 18–22 ms | 23–31 ms | default |
| HEVC (NVENC) | 15–18 ms | 20–23 ms | lower reported latency on this tablet; not a universal codec advantage |
| HEVC Main10 (10-bit) | 16–17 ms | 19–22 ms | similar reported range to 8-bit HEVC |

Of the ~22 ms H.264 figure, the tablet reported about 15 ms from frame
arrival to render callback; the remaining 5–7 ms includes both transport
directions and host queueing. Encoding time was outside this measurement.
On this hardware, `stream_scale = 2` (a quarter of the pixels) brought the
reported p50 from 22 ms to 16 ms at the cost of softer text.

### USB vs Wi-Fi (H.264, quiet link)

| | USB | Wi-Fi, no radio lock | Wi-Fi, with lock (1.0.0+) |
| --- | --- | --- | --- |
| p50 (median of 5-second windows) | 22.0 ms | 32.0 ms | 22.8 ms |
| p95 (median of windows) | 25.3 ms | 113.7 ms | 78.6 ms |
| windows with p95 < 60 ms | 7/7 | 14/73 | 10/31 |
| worst single frame | 32 ms | 5775 ms | 2546 ms |

With the radio lock, this test's median approached USB, while long delays
remained. These observations are consistent with a benefit from reducing
radio power saving; they do not isolate the cause of every delay or establish
that signal strength/router changes cannot help. Other networks and tablets
can behave differently.

### Host CPU

| encoder path | pipeline CPU (helper + encoder) |
| --- | --- |
| ffmpeg child process (default) | ~190 % of a core |
| in-process libavcodec (`--features inproc-encoder`) | ~97 % |

The reported packet-to-acknowledgement latency was similar; this metric does
not establish encoder latency, because its timer begins after encoding.

Historical helper measurement: 96 % of a core before 0.4.0 while the output
was disabled (a poll-loop deadline bug), 1.6 % afterward. Current daemon
lifecycle stops the helper when no display session is needed; the historical
number is not a measurement of current unplugged-daemon CPU usage.

The upstream 1.2.0 follow-up reports FFmpeg 8 encoder-process CPU changing
from ~280 % to ~12 % of a core at a 90 fps target after moving BT.709 tags to
the input to avoid a CPU colour conversion. This is a different version and
measurement from the helper-plus-encoder table; do not compare their totals
as if they were the same workload.

### Frame rate ceiling of the EVDI capture cycle

The helper requests an update, waits for the driver's event, then grabs the
frame into its own buffer. The recorded request wait includes compositor and
driver work; these timers do not prove that every compositor serializes its
next render behind the helper's copy. Upstream measurements at 2960×1848
under continuous motion (the helper reports these stages every 5 seconds):

| half of the cycle | 1.1.0 | 1.2.0 |
| --- | --- | --- |
| compositor answers a request | 9–11 ms | 9–11 ms |
| helper copies the frame (`evdi_grab_pixels`) | 6.3–6.7 ms | 4.0–5.0 ms (huge pages) |
| frames delivered at a 90 fps target | 52–57 /s | 58–63 /s |

At the reported 90 fps target, this setup delivered roughly 60 frames/s.
That does not establish an invariant ceiling for other workloads, drivers or
future changes. `stream_scale` scales after the grab, so it does not reduce
the framebuffer being copied; lowering the virtual resolution does. A
PipeWire/dmabuf path is a proposal whose copy count and performance would
need implementation and measurement.

## Limitations

- One host, one tablet model. Decode/render is a substantial part of the
  measured packet-to-ack interval; total display latency was not measured.
- The historical 90 fps/quality-12 setup is not the fork default: host FPS is
  60, quality is 18, and the app now requests a 60 Hz display mode by default.
- The wire estimate includes both directions of adb/USB and host queueing;
  subtracting independent percentiles cannot isolate individual stages.
- "Windows with p95 < 60 ms" is a coarse stutter indicator, not a standard.
- No measurement yet of AMD/Intel VAAPI encoders or of libx264.

Reports with other hardware are welcome as
[compatibility issues](https://github.com/geraldo-netto/UScreen/issues/new?template=compatibility.yml);
include the exact commit, hardware, encoder/settings, workload and several
`Latency encode→display` log lines. The log label alone does not describe a
reproducible benchmark.
