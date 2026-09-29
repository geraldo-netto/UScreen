# Audio foundation

T717 implements portable Rust audio policy in `blent_config::audio`. It is not a
working microphone/speaker feature yet. Linux native devices, Android capture and
playback, controls, permissions and foreground services remain T718/T719; native
duplex/acoustic acceptance remains T720. All backend capabilities default to
unsupported. No system default device changes or automatic audio startup occur.

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
   UI/native callback threads. No native adapters are supplied in T717.
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
T718/T719 must test partial reads/writes, cancellation and native retirement.

## Version 1 wire format

Protocol version, PCM encoding and 10 ms block duration are fixed by `BLAUD001`.
Integer metadata is big endian; PCM samples are signed **little-endian** 16-bit.
Microphone uses direction 1/mono; speakers use direction 2/stereo, both 48,000 Hz.
The 92-byte handshake must exactly match the authorized session:

| Byte offset | Length | Value |
| --- | ---: | --- |
| 0 | 8 | `BLAUD001` |
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
| 24 | 2 | PCM bytes: exactly 960 microphone or 1920 speakers |
| 26 | 1 | Authorized direction |
| 27 | 1 | Reserved, must be zero |
| 28 | 960/1920 | Exactly one interleaved PCM block |

Timestamps strictly increase after the first frame. No subtraction of host and
tablet absolute clocks is used. Replayed/reordered frames, changed direction or
generation, unknown protocol, wrong format, malformed lengths, truncation and
trailing data are rejected. A sequence gap is accepted with a discontinuity and
flushes queued audio. Parsing uses fixed bounded storage; maximum packet size is
1948 bytes. Authentication is a connection credential check, not encryption or a
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

`adjust_drift` accepts native source/destination frame-counter deltas measured over
the same observation window. It bounds correction to ±1000 ppm, comparing the exact
ratio before integer rounding. Zero windows are rejected; excess drift flushes the
queue and reports an error/discontinuity. A fixed-point linear interpolator applies
the accepted correction, preserving channels and phase across blocks. Rendering
allocates no memory and performs no IO. Native adapters must collect meaningful
counter windows, handle resets and measure real drift; packet arrival alone is
not a native device-clock estimate. This is not a fidelity or AEC effectiveness claim.

## Validation

[September 29 evidence](reviews/2026-09-29-audio-foundation.md) records permanent
normal-suite tests, scoped per-function native Rust coverage and boundary findings.
Native source/sink enumeration, permission, actual acoustic processing, routing,
latency and simultaneous hardware operation remain unvalidated under T718–T720.
