# Benchmarks

Numbers measured by the project, with the method, so they can be reproduced
or argued with. They are from one machine and one tablet; other hardware will
differ.

## Test configuration

| | |
| --- | --- |
| Date | 2026-08-26 to 2026-08-31 |
| UScreen | 1.0.0 – 1.1.0 |
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
| HEVC (NVENC) | 15–18 ms | 20–23 ms | tablet has a dedicated low-latency HEVC decoder |
| HEVC Main10 (10-bit) | 16–17 ms | 19–22 ms | no measurable cost over 8-bit |

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

The app's low-latency Wi-Fi lock fixed the median (Android was dozing the
radio between frames). The tail is the wireless medium and the router, and no
signal strength fixes it — these figures are from a link with none of the
usual excuses. Wi-Fi stays a fallback.

### Host CPU

| encoder path | pipeline CPU (helper + encoder) |
| --- | --- |
| ffmpeg child process (default) | ~190 % of a core |
| in-process libavcodec (`--features inproc-encoder`) | ~97 % |

The reported packet-to-acknowledgement latency was similar; this metric does
not establish encoder latency, because its timer begins after encoding.

Capture helper while the output is disabled (tablet unplugged, or graphics-
tablet mode): 96 % of a core before 0.4.0 (a poll loop with a deadline in the
past), 1.6 % after.

With ffmpeg 8 the encoder process was found at ~280 % of a core: the BT.709
tags passed as output options made ffmpeg convert every frame through RGB on
the CPU. Tagging the input instead (1.2.0) brings it to ~12 % at 90 fps.

### Frame rate ceiling of the EVDI capture cycle

The cycle is serial by the driver's design: the compositor renders the
virtual output and copies it out of the GPU into the EVDI framebuffer, the
helper copies that into its own buffer, and only then does the compositor
start the next frame. Measured on the reference laptop at 2960×1848 under
continuous motion (the helper prints both halves every 5 s):

| half of the cycle | 1.1.0 | 1.2.0 |
| --- | --- | --- |
| compositor answers a request | 9–11 ms | 9–11 ms |
| helper copies the frame (`evdi_grab_pixels`) | 6.3–6.7 ms | 4.0–5.0 ms (huge pages) |
| frames delivered at a 90 fps target | 52–57 /s | 58–63 /s |

So native resolution tops out around 60 frames/s on this hardware whatever
the target is, and the compositor's copy is the part nothing on our side can
shorten. `stream_scale` does not help here (it scales after the grab); a
smaller virtual mode does. A capture path that takes the frame from the
compositor as a GPU buffer (PipeWire/dmabuf) would remove both copies and is
on the roadmap.

## Limitations

- One host, one tablet model. The tablet's decoder dominates the budget, so
  other tablets will land elsewhere; a Snapdragon 8 Gen 2 is a fast one.
- The wire estimate includes both directions of adb/USB and host queueing;
  subtracting independent percentiles cannot isolate individual stages.
- "Windows with p95 < 60 ms" is a coarse stutter indicator, not a standard.
- No measurement yet of AMD/Intel VAAPI encoders or of libx264.

Reports with other hardware are welcome as
[compatibility issues](https://github.com/geraldo-netto/UScreen/issues/new?template=compatibility.yml);
the daemon's `Latency encode→display` log line is all it takes.
