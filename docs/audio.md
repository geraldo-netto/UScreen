# Audio foundation

T717 implements portable Rust audio policy in `blent_config::audio`. T718 adds
Linux/Android microphone transport, native source and controls described below.
T719 adds the selectable speaker sink and Android playback. T720 integrates native
device-clock correction; physical duplex/acoustic acceptance requires a device window.
Unimplemented backend capabilities remain unsupported. No system default device changes or automatic audio startup occur.

Native Rust applications enable the shared crate's `native-entropy` feature
(included by default and by `platform`). A policy-only build compiles without
OS randomness, including on `wasm32-unknown-unknown`; its default audio Start
fails closed. A backend can supply a CSPRNG through `AudioSession::with_entropy`.
That adapter must fill the complete credential buffer or return an error; a
failed fill leaves session state and generation unchanged. Test adapters do not
change the native applications' use of system randomness.

The selected product contract is [microphone/shared audio](reviews/2026-09-29-audio-input.md)
and [tablet speakers](reviews/2026-09-29-audio-output.md). The following describes
implemented shared behavior; those reviews also contain planned native behavior.

## Ownership and adapter integration

Keep one `AudioSession` and native `AudioBackend` owner per direction throughout
an attachment's lifetime. Microphone and speakers have separate state, credentials,
queues, sequence tracking and retirement. A failure in one does not stop the other.
The native implementation must intersect both endpoints' current capabilities before
Start. A supported direction means its 48 kHz s16le channel layout is supported.
Speech capability means processing can be requested, not that AEC works effectively.
Raw and background support are separate capabilities; neither is silently assumed.

1. Explicit user Start calls `AudioSession::start` with validated profile, current
   capabilities, authenticated-tablet status and local monotonic milliseconds.
   It returns a fresh 256-bit CSPRNG credential and increasing generation in an
   opaque `AudioGrant`. Failure leaves the session unchanged. Settings Apply,
   application launch and reconnect must never call Start implicitly.
2. Deliver `AudioGrant::hello()` only over the existing authenticated control path.
   Start the adapter asynchronously; it owns devices, paths, permissions, task
   cancellation and IO deadlines. The trait enqueues work rather than waiting on
   UI/native callback threads. T717 itself supplies no native adapters; the T718/T719 integrations are described below.
3. A fixed handshake must match the grant before `connected` admits Streaming.
   The startup deadline is 5 seconds. Failed negotiation cannot acquire ownership
   or extend the deadline. Do not log grants/handshakes or persist credentials.
4. Use one authenticated connection per direction. Validate the fixed frame header
   with `FrameReader::payload_bytes` before allocating or reading its payload;
   `decode`/`AudioSession::receive` validates the complete frame atomically. A native
   reader may maintain its own FrameReader obtained from the grant alongside the
   session's independent validator. Never reset it between packets.
5. Poll `tick` from the owning worker even without incoming bytes. Streaming has a
   250 ms complete-frame deadline, refreshed only by accepted frames. Deadline or
   clock regression closes admission and clears PCM. It returns the generation
   whose adapter must be stopped. Partial socket reads cannot keep a session alive.
6. User Stop calls `stop`. Permission/focus loss, disconnect and asynchronous native
   failure call generation-bound `cancel`; late callbacks cannot cancel replacements.
   `receive` errors on the current generation also enter Stopping; the worker must
   retire its adapter when observing this state. Native retirement completes off
   the UI thread, followed by `retired(generation)`. Keep Stopping if retirement is
   stuck; do not release the ownership barrier merely because a timer expires.
7. Replacement Start is rejected until retirement completes. Reconnect does not
   resume audio. Stale render calls emit silence and cannot stop a newer session.
   Reset the whole owner only after all its native workers have retired.

IO workers must impose cancellable deadlines for handshake, header, payload and
writes. Device callbacks must use adapter-owned rings, without socket IO or waiting.
This domain layer performs no IO and cannot enforce an adapter's native shutdown.
T718/T719 retain tests for partial reads/writes, cancellation and native retirement.

## Versioned wire format

The grant selects `BLAUD001` (legacy PCM) or `BLAUD002` (PCM plus native counters).
PCM encoding and 10 ms block duration are fixed in both versions.
Integer metadata is big endian; PCM samples are signed **little-endian** 16-bit.
Microphone uses direction 1/mono; speakers use direction 2/stereo, both 48,000 Hz.
The 92-byte handshake must exactly match the authorized session:

| Byte offset | Length | Value |
| --- | ---: | --- |
| 0 | 8 | `BLAUD001` or `BLAUD002`, matching the selected session |
| 8 | 64 | Fresh credential, lowercase hexadecimal ASCII |
| 72 | 8 | Nonzero session generation |
| 80 | 1 | Direction: 1 microphone, 2 speakers |
| 81 | 1 | Channels: 1 microphone, 2 speakers |
| 82 | 4 | Sample rate: 48000 |
| 86 | 2 | Frames per block: 480 |
| 88 | 1 | Processing: 1 speech, 2 raw |
| 89 | 1 | Background consent: 0 or 1 |
| 90 | 2 | Queue target: 20–200 ms, steps of 10 |

Each packet starts with a 28-byte header:

| Byte offset | Length | Value |
| --- | ---: | --- |
| 0 | 8 | Authorized generation |
| 8 | 8 | Sequence: starts at zero, strictly increases, `u64::MAX` rejected |
| 16 | 8 | Sender monotonic timestamp in microseconds |
| 24 | 2 | Payload bytes: 960/1920 for v1, 984/1944 for v2 |
| 26 | 1 | Authorized direction |
| 27 | 1 | Reserved, must be zero |
| 28 | 0/24 | Native clock prefix, present only in v2 |
| 28/52 | 960/1920 | Exactly one interleaved PCM block |

The v2 prefix contains three big-endian unsigned 64-bit values: native epoch,
frame position normalized to 48 kHz, and native monotonic nanoseconds. All-zero
bytes explicitly mean unavailable. Otherwise epoch/time must be positive and all
three fields must fit a signed 64-bit value for the Android contract. Invalid
counters reject the complete packet without advancing receiver sequence state.

Timestamps strictly increase after the first frame. No subtraction of host and
tablet absolute clocks is used. Replayed/reordered frames, changed direction or
generation, unknown protocol, wrong format, malformed lengths, truncation and
trailing data are rejected. A sequence gap is accepted with a discontinuity and
flushes queued audio. Parsing uses fixed bounded storage; maximum packet size is
1972 bytes (1948 for v1). Authentication is a connection credential check, not encryption or a
per-packet MAC. The wire is intended for the already-authorized ADB connection;
other transport security requires an explicit adapter contract.

## Buffering, drift and silence

`AudioProfile` defaults to speech, foreground-only and a 40 ms application queue
target. The target accepts 20–200 ms in 10 ms steps. Lower values reduce startup
buffering but tolerate less jitter. Higher values add lag. This budget does not
include Android, PipeWire, device or transport buffering and is not a measured
end-to-end latency promise.

`PcmQueue` preallocates space for twenty complete 10 ms blocks, independent of the
requested prefill target. Rendering waits for that target after Start/underflow.
Overflow drops the oldest block (including any unconsumed remainder), preserving
channel alignment and reporting discontinuity. Blocks older than 200 ms on the
receiver's local clock are discarded. Underflow returns an entire silent block;
it never repeats old speech. Stop clears pending samples immediately. Queue and
render calls reject backwards clocks and incompatible sizes/channel layouts.

`adjust_drift` accepts native source/destination frame-counter deltas normalized
to the same duration. Rust and Android form independent two-to-five-second native
windows and compare `source_frames × destination_ns` against
`destination_frames × source_ns`. They never subtract absolute endpoint clocks or
estimate a native rate from packet arrivals. Completed windows expire after three
seconds on the receiver's local clock; repeated timestamps cannot refresh them.

PipeWire reads the graph driver's SPA position, rate and monotonic timestamp;
Android reads AudioRecord/AudioTrack timestamps. These are native pipeline/graph
observations, not guaranteed physical hardware oscillator measurements. Clock
availability and accuracy depend on the route/backend. Epoch, frame/time reset,
stalls, invalid windows and excess drift reset correction. Missing timestamps
leave ordinary bounded playback at nominal rate and report drift as unmeasured.

Correction is limited to ±1000 ppm using the exact ratio before integer rounding.
A fixed-point linear interpolator preserves channels and fractional phase across
blocks. The Rust render callback allocates no memory and performs no IO; Android
interpolation uses bounded arrays on its IO worker. Focus flushes and AudioTrack
replacement advance the playback epoch. Existing queue age, underflow and
retirement bounds remain in force. Microphone correction/unavailable status is
logged numerically outside the native callback; Android speaker status reports
it alongside native buffer/route diagnostics. No speech or PCM content is logged.

The [native integration evidence](reviews/2026-09-30-audio-clock.md) covers synthetic
rates, adapters and private PipeWire graphs. This is not an acoustic latency,
fidelity, physical duplex or AEC effectiveness claim.

## Validation

[September 29 evidence](reviews/2026-09-29-audio-foundation.md) records permanent
normal-suite tests, scoped per-function native Rust coverage and boundary findings.
[T718 evidence](reviews/2026-09-29-audio-microphone.md) records native source/transport
regressions and bounded physical checks. [T719 evidence](reviews/2026-09-29-audio-speakers.md)
records private sink/transport tests and connected-tablet playback/lifecycle.
Intentional speech remains T718; physical duplex, route/USB transitions, measured
acoustic effectiveness and delay remain T720.

## Linux/Android microphone integration (T718)

The implementation now includes microphone Start/Stop in the host **Audio** tab,
`blent audio --direction microphone`, and Android settings. Select **Blent
Microphone** in the desktop application's input chooser after Start. The source
is owned by that session and removed at Stop. Existing default devices are not
changed. Microphone capture starts only after Android permission and authenticated
host readiness; app launch, Apply and reconnect never start audio.

Host settings: speech/raw processing, application buffer 20–200 ms (default 40),
optional tablet serial and background request. Apply saves preferences without
restarting display sharing. Android settings: follow host/speech/raw override,
gain 0–200% (100% default), built-in microphone preference and explicit background
consent. Changing tablet settings ends the active session. Foreground-only capture
stops when hidden; background capture uses a microphone foreground service with
an ownership-bound Stop notification. Permission loss, native errors, Android
silencing, an observed route change or transport loss end the session. Raw mode
requires advertised native unprocessed support. Speech requests Android AEC and
reports availability, without claiming effective acoustic echo removal.

Linux uses an isolated `blent-audio` PipeWire adapter, built with
`--features blent/native-audio`. Source builds need PipeWire/SPA development
headers and libclang (`libpipewire-0.3-dev`, `libspa-0.2-dev`, `libclang-dev` on
Debian-family systems). Packaged builds include the helper. Audio needs the host's
PipeWire client library, SPA modules and running user server; the AppImage uses
that native runtime to match its modules. Missing audio dependencies fail Start
without preventing display sharing. PulseAudio-only, Windows and macOS audio
backends remain unsupported.

The DUMP-protected Android receiver admits an invitation over the authorized ADB
connection. An authenticated 76-byte `BLAUREQ2` bootstrap requests the clocked
protocol; the host still accepts legacy `BLAUREQ1`. The bootstrap carries the invitation
credential (64 hex bytes), capability bits (speech/raw/background), AEC-enabled
flag, direction and effective processing. Human consent has a separate 90-second
budget; it does not consume native startup's five seconds. The host echoes the
invitation credential followed by the 92-byte grant after PipeWire readiness.
Android gives that complete nonce/grant one five-second deadline; partial reads
cannot renew it ([T724 regression evidence](reviews/artifacts/2026-09-29-audio-grant-deadline/README.md)).
Native microphone input preserves sequence-gap discontinuities; the callback uses
only its bounded queue, expires old data and emits silence on underflow. It never
waits on the transport.

Native device-counter integration is implemented for the clocked protocol. Legacy
peers carry no native counters and remain unmeasured. Deploy matching host/Android
builds for v2; an older host does not recognize the new request. Physical duplex
drift/latency and route acceptance remain T720.

## Linux/Android speaker integration (T719)

Start **Speakers** in the host Audio tab or run `blent audio --direction speakers`.
Select **Blent Speakers** in the desktop application's output selector. The owned
PipeWire `Audio/Sink` appears only for this session. Blent leaves existing defaults
and application routes unchanged; it does not mirror the current system mix.
Microphone and speakers have separate controls, saved settings, authentication,
transport, queues and failure ownership. Apply never starts either direction.

Android provides processing override, volume 0–100% for this track, built-in speaker
preference and explicit background consent. It needs no microphone permission for
playback. Speech requests communication playback; Raw requests media playback.
Neither establishes effective acoustic echo cancellation. Actual native route,
buffer frames, played frames and underruns are reported separately from the
application queue setting. An observed route change stops playback and requires
explicit Start; a dead AudioTrack is recreated within its existing authorization,
discarding the interrupted block. Native buffer minima can exceed the requested
application buffering.

Audio focus denial prevents playback. Transient loss, including duck requests,
pauses and flushes sound; focus regain can resume only the same active session.
Permanent focus loss stops it. Foreground-only playback stops when the app hides.
Consented background playback owns its mediaPlayback foreground service and Stop
notification independently of microphone, camera and display services. Changing
tablet settings stops that direction. App launch or reconnection never resumes it.

The PipeWire callback copies bounded stereo chunks into its queue without pipe IO
or waiting for a mutex. It rejects corrupt/unknown chunk flags, treats EMPTY as
silence, and honors SPA's modulo offsets and clamped sizes. Incomplete chunks
expire after a 200 ms capture stall. A
worker dequeues whole blocks without adding another prefill, emits silence on
underflow, and paces transfer at 10 ms. Android validates each complete frame within
one 250 ms deadline, keeps at most twenty blocks, and applies the requested prefill
after Start/underflow. Gaps and overflow discard stale samples. AudioTrack short
writes share one 250 ms complete-write budget; partial progress cannot renew it.
Stop, transport loss or PipeWire server loss removes the owned sink and releases
playback. Native counter correction follows the policy above. These limits bound
application backlog; they do not establish measured acoustic latency.
