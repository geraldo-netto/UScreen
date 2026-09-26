# T608 — selective camera transport research completed

Keep TCP/ADB and the existing latest-decoded-frame publication. T607 found no
useful USB gain from larger chunks; T608 found no reason to add a datagram or
WebRTC dependency for that route. The implemented improvement for an overloaded
route is already available: lower the configurable camera bitrate. A measured
1 Mb/s source restored freshness on a 2 Mb/s test link; changing the transport
alone did not. The normal 3 Mb/s USB camera default remains unchanged.

The maintainer explicitly requests selective reuse, not implementation of every
protocol or a full WebRTC stack. Prefer improving existing bounded queues and
frame handling where sufficient. Evaluate total maintained code, dependency
footprint, attack surface, lifecycle states and measured CPU/memory/latency.
A mature library may replace riskier custom logic, but a larger stack needs
requirements and measurements that justify its cost. Fewer lines alone are not
proof of security or speed. A valid research outcome is keeping the current
transport and adopting no new dependency.

## Implemented route

`CameraWire.connect` uses a loopback TCP socket with `TCP_NODELAY`, a requested
128 KiB send buffer and three-second I/O timeout. `CameraWire.packet` emits a
length-prefixed encoded packet through bounded Okio segments and flushes at the
packet boundary. `host/src/camera/bridge.rs` owns a temporary `adb reverse tcp:0`
route. Host decoding and virtual-camera publication follow that transport.
This is raw framed TCP, not HTTP or WebSocket.

The attached tablet reports 480 Mb/s USB and 512-byte maximum bulk payloads on
both ADB endpoints. Host Wi-Fi MTU is 1500; loopback MTU is 65536. These are
separate limits from Okio's 8192-byte segments and application packet lengths.
ADB adds its own framing and flow control. Its documented reverse endpoint
choices do not include UDP; placing datagrams inside a reliable ADB stream
would retain the outer stream's ordering.
[ADB command reference](https://android.googlesource.com/platform/packages/modules/adb/+/refs/heads/main/docs/user/adb.1.md).

## Candidate lessons

| Design | Potential benefit | Required work / limitation |
| --- | --- | --- |
| Current TCP/ADB with bounded queues | Simple reliable USB path; no new pairing route | Measure queue age and encoder/decoder delay before blaming TCP. Byte delivery alone does not bound frame age. |
| WebSocket | Message framing and browser-friendly control | Standard WebSocket is TCP-based; it retains ordered-delivery blocking after loss. No demonstrated webcam improvement from wrapping existing bytes. |
| Direct RTP/SRTP over UDP | Can stop waiting for expired media packets | Fragmentation/reassembly, sequencing, deadlines, codec recovery, congestion control, authentication and a direct route are needed. |
| WebRTC | Established secure real-time media, feedback and network adaptation | More dependencies and session machinery; benchmark CPU/memory/startup and codec compatibility. UDP is a common route, not a guarantee of every WebRTC connection. |

[WebSocket](https://www.rfc-editor.org/info/rfc6455/) supplies TCP-based framing.
[WebRTC's RTP transport](https://www.rfc-editor.org/rfc/rfc8834.html) includes
feedback, selective retransmission and congestion adaptation. A late packet can
be worthless, but an important missing reference packet can be cheaper to repair
than replacing decoder state. UDP therefore moves recovery policy into the media
system; it does not eliminate loss handling.

[H.264 RTP packetization](https://www.rfc-editor.org/rfc/rfc6184.html) supports
fragmented NAL units. Lost fragments and lost reference pictures need deliberate
recovery. Dropping an obsolete decoded output is different from dropping an
arbitrary encoded reference picture; proposed frame-dropping rules must preserve
this distinction.

Use inspectable implementations:

- [scrcpy](https://raw.githubusercontent.com/Genymobile/scrcpy/master/doc/video.md)
  supports Android camera capture and V4L2 sinks; its documentation exposes an
  explicit latency-versus-jitter buffering choice, with no added video buffer
  by default. Inspect capture, decode and sink queues as well as transport.
- [GStreamer rtpjitterbuffer](https://gstreamer.freedesktop.org/documentation/rtpmanager/rtpjitterbuffer.html)
  reorders packets, bounds waiting for missing packets and can request
  retransmissions. Its configurable timing and loss notifications provide a
  concrete model for testing deadlines and recovery.
- WebRTC specifications and source are suitable references for pacing, feedback
  and congestion response. No claim is made about Skype's proprietary code.

## Completed isolated replay

The retained benchmark uses the T607 encoded fixture: 300 frames, 1280×720,
30 FPS, H.264 baseline, one-second IDR interval, no B frames, nominal 3 Mb/s.
This is the camera profile, separate from the tablet's 1280×800 display geometry.
Three interleaved trials per transport/scenario run in disposable user/network
namespaces. A namespace-identity guard refuses live-network modification.
Loopback MTU is 1500, TCP MSS 1200, segmentation/coalescing offloads disabled.
No tablet or host live interface was subjected to impairment.

The three scenarios are clean, 12±6 ms delay / 0.5% packet loss / 10% reordering /
20 Mb/s, and 10±3 ms delay / 2 Mb/s bandwidth / 64-packet queue. Kernel `netem`
applies real packet loss and TCP retransmission, not a modeled TCP delay.
Random seeds are unsupported by this installed iproute2; all repetitions and
qdisc counters are retained. These are bounded mechanism tests, not a Wi-Fi
hardware campaign or a model of the USB bus's error recovery.

TCP uses production framing, an initial 74-byte authenticated greeting,
TCP_NODELAY, 8 KiB writes and a requested 128 KiB send buffer. The experimental
UDP probe uses 1,152-byte fragments with a per-run HMAC key, bounded eight-frame
reassembly, a 150 ms deadline, complete-frame validation and an IDR recovery
rule. It is **not RTP, SRTP or WebRTC**, supplies no confidentiality/pairing,
and has no retransmission, feedback, pacing controller or bitrate adaptation.
It is isolated benchmark code, never packaged in the product. Comparisons do
not establish the performance of a mature WebRTC implementation.

| Scenario, source | TCP decoded / generated | UDP decoded / generated | TCP receiver-age p95 / p99, median ms | UDP receiver-age p95 / p99, median ms |
| --- | --- | --- | ---: | ---: |
| Clean, 3 Mb/s | 300/300 each trial | 300/300 each trial | 0.18 / 0.22 | 0.41 / 0.69 |
| Loss/jitter, 3 Mb/s | 300/300 each trial | 132/300, 97/300, 153/300 | 71.64 / 100.51 | 33.96 / 44.27 |
| 2 Mb/s pressure, 3 Mb/s | 300/300 each trial, all late | 0/300 each trial | 6,742 / 6,980 | No decodable frames |
| Same pressure, **1 Mb/s** | **300/300 each trial, all within 150 ms** | **300/300 each trial, all within 150 ms** | **63.13 / 90.53** | **63.19 / 87.87** |

Age starts at the scheduled source-frame time and ends at complete reception;
it includes sender backlog. It excludes sensor, codec execution, V4L2 and video
application presentation. These are **not glass-to-glass latency** numbers.
The pressure run's large TCP age is therefore explicitly visible even though
all bytes eventually arrive. Real camera/MediaCodec backpressure can behave
differently from a pre-encoded, scheduled replay.

![Latency and completeness](artifacts/2026-09-26-camera/graphs/transport-tradeoffs.png)

Independent FFmpeg decoding verified every accepted frame pixel-for-pixel
against its corresponding clean encoded reference. The UDP loss cases require
1.73–2.00 seconds to span their longest unavailable frame runs; a low p95 for
surviving frames must not hide those freezes. Holding the last accepted frame
for missing pictures yields loss-only luma PSNR 24.18–25.67 dB; TCP has no
loss-only image difference. Under pressure, UDP accepts no usable IDR chain,
so the loss-only blank-output proxy is 5.00 dB. This quality proxy excludes
transport delay; latency is reported separately. `null` PSNR in raw data means
zero error/infinite PSNR, not a missing quality measurement.

The 1 Mb/s follow-up scales FFmpeg bitrate/maxrate and the one-second VBV
budget together, retaining geometry, frame rate, preset and route conditions.
It tests a lower source-rate profile, not an isolated MediaCodec control or a
bit-identical encoder configuration with only one flag changed. All 1,800 frames across six
trials arrive within the deadline and decode exactly. Its decoded luma differs
from the 3 Mb/s encoded reference by 35.23 dB PSNR. That is an **encoding quality
tradeoff**, not a score against the original sensor image or a claim that lower
bitrate improves picture quality.

![Rate budget improvement](artifacts/2026-09-26-camera/graphs/transport-bitrate.png)

## CPU, memory and the smallest useful change

The complete Python transport probes use about 22–23 MiB peak process RSS.
At 3 Mb/s, TCP uses 0.032–0.048 CPU seconds per trial and UDP 0.087–0.129 seconds;
UDP includes Python fragmentation, HMAC and reassembly. This is attribution for
the experiment, **not a Rust/Android or WebRTC CPU comparison**. Exact CPU/RSS,
peak reassembly and write-stall measurements are retained for each run. The
production-path Android/ADB costs are measured separately by T607.

The host already publishes decoded camera frames through a Tokio watch slot
(`decoder::transfer` → `outputs::write_frames / current_frame`), so a slow output receives the
latest decoded picture. Existing T539 tests cover stale/inactive blanking.
Adding another latest-frame queue there duplicates working policy. Dropping an
arbitrary *encoded* picture before decoding would instead damage reference
chains; the UDP experiment makes that cost concrete.

The smallest response to a persistently bandwidth-limited camera route is to
use the existing camera bitrate control. The measured USB route carries roughly
220 Mb/s in T607's synthetic receiver, far above the 3 Mb/s camera source;
there is no evidence to lower its default or add new transport code. Sustained
camera-rate feedback/age telemetry is recorded as deferred T614, conditional on
a real route needing it. The experiment does not reopen the declined T382
multi-device/large-machine campaign.

A future direct route would need authenticated pairing/confidentiality,
anti-replay bounds, explicit sequence/timestamps, fragment limits, deadlines,
IDR feedback, pacing and congestion response. Reuse a maintained implementation
when those requirements exist; do not ship this research HMAC probe as a media
protocol. WebSocket would retain TCP ordering. WebRTC may solve those broader
requirements, but has not been benchmarked here and is not rejected universally.

## Reproduce, validation and saved data

```sh
python3 scripts/benchmarks/camera-fixture.py --output /tmp/camera-3000
python3 scripts/benchmarks/camera-transport-run.py --fixture /tmp/camera-3000/camera-replay.bin --output /tmp/transport-3000
python3 scripts/benchmarks/camera-transport-quality.py --fixture /tmp/camera-3000/camera-replay.bin --results /tmp/transport-3000/results.json --work /tmp/quality-3000 --output /tmp/quality-3000.json
# Repeat with a new fixture directory and --bitrate 1000, then --scenarios pressure.
python3 -m unittest discover -s scripts/tests -p test_camera_transport_research.py
```

Four permanent benchmark-validation tests cover tampered/truncated datagrams,
fragment ordering, bounded in-flight storage, expiry and keyframe recovery.
The existing production ownership, authentication, packet-bounds, cancellation
and stale-output regressions remain intact. No production transport changed,
so no new product recovery protocol is claimed.

[Compressed full results and compact summary](artifacts/2026-09-26-camera/t608/)
include source/fixture identities, all repetitions, kernel/qdisc settings, frame
ages, decoded quality, CPU and memory. Exact packet fixtures and per-run files
are also retained under `~/.local/share/blent/profiles/2026-09-26-camera/t608/`.
[SVG/PNG graphs](artifacts/2026-09-26-camera/graphs/) regenerate with
`scripts/benchmarks/camera-transport-graphs.py`. Sensors stayed off for all
research; the profile app and allocated ADB replay mapping were retired.
