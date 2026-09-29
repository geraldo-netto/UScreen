# T545: computer output on tablet speakers

Status: requirements research completed; playback is not implemented. The
maintainer selected a **Blent Speakers** computer output, independent Start/Stop,
unchanged existing defaults, and simultaneous use with **Blent Microphone**.
This extends the [T544 shared contract](2026-09-29-audio-input.md); camera acceptance
T621 has passed; remaining physical USB acceptance T540 is an implementation
prerequisite.

## Routing and device behavior

Publish a selectable Linux PipeWire `Audio/Sink` named Blent Speakers, owned by
the active session. Applications choose it through their existing output selector
or desktop audio controls. Do not move existing streams, change the default sink,
or capture the current system mix implicitly. Mirroring an existing output is a
different opt-in route and is outside the selected first version. Avoid connecting
the microphone source to the speaker sink automatically; that would create feedback.

Use the same native stream adapter and bounded PCM block policy as the microphone,
with the data direction reversed. The graph callback supplies stereo frames to an
owned ring; a worker transmits them independently of microphone reads. Server
restart, removal, Stop or transport loss retires the owned sink and cancels IO.
Expose state and recovery action; require explicit Start after session loss.
Keep desktop consumers' fallback routing visible as OS policy rather than claiming
Blent controls every application's response to a removed sink.

PipeWire documents creating virtual sources/sinks through
[loopback streams](https://docs.pipewire.org/page_module_loopback.html) and exporting
a sink to a [pipe tunnel](https://docs.pipewire.org/page_module_pipe_tunnel.html).
A direct stream adapter gives explicit process ownership; a pipe-sink prototype
must account for full FIFOs and module removal. These upstream mechanisms support
the design choice; they do not establish compatibility with the installed 1.0.5
server or every desktop consumer. Add native tests before marking support available.

## Android playback and shared ownership

Feed negotiated PCM into streaming
[AudioTrack](https://developer.android.com/reference/android/media/AudioTrack).
Honor minimum buffer sizes and short writes; track playback progress and underruns.
A dead AudioTrack requires recreation within the still-authorized session, with
old buffers discarded. Keep native stop/release off the Activity thread and ensure
late completion cannot retire a newer track. Display and camera sessions must
survive audio-only Stop/failure.

Request [audio focus](https://developer.android.com/media/optimize/audio-focus)
before playback. Focus denial, transient loss and permanent loss must have explicit
states; proposed initial policy pauses/mutes on transient loss, resumes only within
the same active session on regain, and stops on permanent loss. Flush stale queued
sound before resuming. Android API 35+ focus requests require the foreground app
or an eligible foreground service. Hidden playback uses an explicitly consented
mediaPlayback foreground service with notification Stop; microphone service and
permission ownership remain separate, even during duplex operation. Foreground-only
playback stops when hidden. Do not assume the display connectedDevice service alone
satisfies every audio lifecycle contract.

Default route is the tablet's built-in speaker. Enumerate and report actual routing;
Android routing requests are not proof of the selected physical route. Headset or
Bluetooth changes must trigger a visible transition and fresh capability/buffer
checks. Do not silently change the computer's unrelated Bluetooth/HDMI streams.
Bluetooth output latency and platform buffer minima may exceed the requested
application queue budget; report actual observations separately.

## Duplex speech and optional raw mode

Both directions can run together, but maintain independent authorization, queues,
statistics and failure state. Shared Android audio-mode/routing/effect ownership
must be generation-safe and reference counted: stopping speakers must not release
a microphone resource, and vice versa. Restore any app-owned mode only if still
owned; do not overwrite a route chosen by another app.

Speech mode coordinates communication playback with microphone capture and the
platform AEC effect. Android's
[AcousticEchoCanceler contract](https://developer.android.com/reference/android/media/audiofx/AcousticEchoCanceler)
attaches an effect to the capture session; it does not guarantee effective acoustic
cancellation for this tablet and route. Validate the playback reference, double-talk,
route changes and AEC control loss. If unavailable, show that limitation; do not
claim echo-free speakerphone behavior. A software AEC dependency needs a separate
measured decision if the native path cannot meet the contract.

Raw microphone mode disables optional Blent processing where supported. Clearly
show that simultaneous raw microphone and loudspeaker playback can contain acoustic
echo; raw does not mean echo cancellation. Playback is stereo media PCM unless the
native communication route requires an explicitly negotiated change. Neither mode
promises bit-perfect physical capture/playback or synchronized camera audio.

## Implementation and permanent acceptance

T719 follows T717/T718: Linux sink, Android playback/focus adapter and host controls.
Reuse the authenticated framing, independent bounded IO, silence/flush policy,
48 kHz s16le format and configurable queue policy from T544. Extend permanent tests
for partial writes, invalid channel/format combinations, burst/slow receivers,
focus denial/loss/regain, device replacement, stale callbacks, shutdown and route
changes. Every production function retains the >=80% individual coverage gate.

T720 follows both directions. Native acceptance must show desktop applications
can select each endpoint independently, Stop silences output, USB removal/reconnect
cannot replay old audio, one-direction failure leaves the other and display/camera
owned correctly, and duplex speech/raw status matches actual processing. Use a
bounded authorized acoustic test window; retain numerical measurements, not speech
recordings. Quantify end-to-end delay and drift under an agreed local workload;
do not infer playback latency from network arrival or queue settings. No broad
capacity campaign is required.

No system audio settings were modified during this research. Windows and macOS
backends remain outside this Linux/Android implementation plan.
