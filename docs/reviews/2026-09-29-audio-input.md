# T544: tablet microphone and shared audio contract

Status: requirements research completed; audio is not implemented. On September
29 the maintainer selected **Blent Microphone** and **Blent Speakers** as ordinary
selectable computer devices, independent explicit Start/Stop, unchanged existing
defaults, simultaneous operation, speech processing with optional raw mode.
T621 native consumer and [T540 physical USB acceptance](2026-09-29-usb-camera-readiness.md)
have passed; shared implementation T717 is ready. T545 builds on the common
contract below; display and camera transports retain independent ownership.

## Current implementation and local evidence

`common/src/camera.rs` and `host/src/camera_control.rs` provide a useful manual
session/backend pattern. `gui/src/camera_settings.rs` owns host configuration;
Android's camera owner separates admission, cancellation and native retirement.
Reuse these responsibilities, not camera H.264 framing or camera service types.
`android/app/src/main/AndroidManifest.xml` has neither RECORD_AUDIO nor a microphone
service. No Blent source/sink or audio transport exists.

Read-only inspection found PipeWire 1.0.5 with its PulseAudio compatibility server
(protocol 35), `pactl`, `pacat` and `pw-cat` on this Linux host. The tablet reports
Android API 36. This establishes installed tooling, not microphone, echo-control,
raw-capture or low-latency support. No microphone was opened; no audio recorded.

## Linux backend selection

Recommend a PipeWire native stream adapter publishing an `Audio/Source` node named
**Blent Microphone**. Android PCM enters its bounded input ring; the graph callback
copies available samples and fills underflow with silence. Keep allocation,
network IO, waiting and module management outside the real-time callback. Session
ownership must remove only its own node; never select it as the system default.
A native stream can own source lifetime without persistent server configuration.

PipeWire's documented [pipe tunnel](https://docs.pipewire.org/page_module_pipe_tunnel.html)
can create a source from incoming FIFO samples or a sink that exports samples.
Its Pulse compatibility [pipe source](https://docs.pipewire.org/page_pulse_module_pipe_source.html)
and [pipe sink](https://docs.pipewire.org/page_pulse_module_pipe_sink.html) expose
format/rate/channel settings. These are feasible prototype/fallback adapters,
but FIFO capacity, zero-reader behavior and server-owned module cleanup require
validation. Prefer direct stream ownership for production. Do not assume a monitor
source is a microphone device in every desktop consumer. Native PulseAudio-only
support requires its own tested adapter; ALSA-only hosts remain visibly unsupported.
No Windows or macOS implementation is included in this work.

## Shared domain and transport proposal

Create portable `AudioProfile`, `AudioCapabilities`, `AudioSession` and backend
interfaces before either direction. Settings describe directions, sample format,
processing, requested buffering and background consent; OS node IDs and paths
stay in adapters. States: Stopped, Starting, Streaming, Stopping, Failed, with
separate state for each direction. Starting requires an explicit user action and
an authenticated tablet. Settings Apply never starts capture; launching either
application never restores an old microphone session. Stop from either endpoint,
permission revocation or USB/control loss closes admission immediately, clears
queued samples and retires native resources off the UI thread. Reconnect requires
a fresh Start. Stale callbacks cannot operate on a replacement generation.

Use separate authenticated ADB connections for audio, with independent reader and
writer tasks. Camera/video backpressure must not block audio cancellation. Negotiate
protocol version, direction, PCM format and channel layout before payload allocation.
Bind a fresh credential to the session generation; reject a different direction,
unknown version, invalid sequence, malformed lengths and stale generation. Do not
reuse the camera's packet-acceptance ACK as a playback/capture latency claim.

Proposed initial wire format: PCM signed 16-bit little endian, 48 kHz; microphone
mono, speakers stereo. A 10 ms block carries 480 frames: 960 bytes uplink and 1920
bytes downlink. Payload alone is 96,000 and 192,000 bytes/s respectively (2.304 Mbit/s
duplex), calculated rather than measured. PCM avoids codec delay and simplifies
bounded tests. Advertise actual native formats; use an explicitly owned resampler
or reject unsupported negotiation, never silently reinterpret samples.

Proposed controls: target application queue 40 ms, selectable 20–200 ms in 10 ms
steps, hard cap 200 ms per direction. Lower values trade dropout tolerance for
latency; larger values add lag and memory. Reject negative, overflow, non-aligned
and out-of-range values. Queue overflow discards oldest complete blocks with a
reported discontinuity; underflow emits silence, never repeats old speech. Bound
all socket/native waits and reset sequence/buffer state across Start/Stop. Device
clocks differ: bounded adaptive resampling corrects long-term drift, with a small
explicit correction range and discontinuity on excess drift. Packet timestamps
use each sender's monotonic clock; never subtract unsynchronized host/tablet clocks.
A future compressed network mode requires separate measurement and negotiation.

## Android capture and consent

Use [AudioRecord](https://developer.android.com/reference/android/media/AudioRecord)
behind an adapter. Check initialization/minimum buffer requirements, handle short
reads and dead objects, and observe routing changes. Android can silence or deny
concurrent recording; surface that state instead of reporting healthy capture
([audio input sharing](https://developer.android.com/media/platform/sharing-audio-input)).
Request RECORD_AUDIO only when microphone Start is requested. Speakers alone need
no microphone permission. Default foreground-only mode stops when hidden; an
explicit continue-while-hidden option requires a microphone foreground service,
notification with Stop, and tightly scoped wake-lock ownership. Android's
[microphone service rules](https://developer.android.com/develop/background-work/services/fgs/service-types#microphone)
require runtime permission and restrict starting capture from the background.
A host request must bring the user to a visible consent path rather than bypass it.

Speech mode requests voice-communication capture and checks
[AcousticEchoCanceler](https://developer.android.com/reference/android/media/audiofx/AcousticEchoCanceler)
availability, creation and enabled state. Treat AEC availability as a capability,
not proof of effective cancellation. Raw mode requests unprocessed capture only
where the device supports it; report unsupported rather than claiming fidelity
for a processed fallback. Device-specific processing, gain and routing need
native verification. T545 defines the paired playback reference and duplex gate.

## Dependency-ordered implementation and acceptance

1. T717: shared lifecycle, authenticated framing, bounded queues and adapter contracts;
   camera prerequisites T621/T540 passed. Permanent fake-clock/fake-device tests must cover both
   directions before native integration, including invalid values, truncation,
   overflow, replay, drift boundaries, cancellation and generation races.
2. T718: Android microphone permission/lifecycle plus Linux selectable source and
   host controls. Requires T717. Validate real desktop consumer enumeration,
   intentional speech, silence on Stop/loss, permission denial/revocation,
   rapid Start/Stop, route changes and fresh selection after reconnect. Keep
   recordings out of evidence; retain counters, route identities and timings.
3. T545/T719: add selectable speaker output using the same transport/session policy.
4. T720: validate simultaneous speech/raw operation, AEC and timing on this tablet.
   Measure latency with an actual loopback/correlated stimulus; receipt timings
   alone do not measure microphone-to-consumer or speaker presentation latency.

Each maintained function needs its own >=80% executable-line coverage, retained
bounded invalid-input tests and native adapter acceptance. Initial timing controls
are design proposals, not measured latency guarantees. The scope excludes an
unsolicited broad multi-tablet performance campaign.
