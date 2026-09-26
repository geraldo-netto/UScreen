# T608: camera transport research starting point

No camera transport change is justified by measurements yet. T594 measured
Android packet allocation and copying, not live camera latency or TCP loss.
T607 first separates local write-chunk costs from transport effects. T608 then
compares loss recovery, queue age and latency on an isolated replay route.
Camera capture remains off unless explicitly requested.

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

## Next experiment and acceptance

Replay the same encoded camera-like sequence with the sensor off. Compare
loss-free USB separately from an isolated network route with controlled loss,
reordering, jitter and limited bandwidth. Record sender/receiver queue age,
delivery completeness, frame age, p95/p99 latency, recovery time, quality,
CPU, allocations and retained bytes. Separate transport delay from codec and
virtual-camera buffering. Avoid changing the user's live network for injection.

Keep current transport unless measured improvement warrants a prototype. Any
production replacement needs explicit pairing/authentication, bounded packet
sizes and reassembly, deadline cancellation, reconnect ownership, codec recovery
and permanent red-before-green regressions for discovered behavioral bugs.
Place transport policy behind portable interfaces with native backend adapters.
