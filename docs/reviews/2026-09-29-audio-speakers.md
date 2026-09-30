# T719: Linux/Android speaker integration

The host publishes a selectable **Blent Speakers** PipeWire sink, independently
of **Blent Microphone**. Android receives authenticated 48 kHz stereo PCM and
plays it through its own AudioTrack/focus owner. CLI and GUI Start/Stop, passive
saved preferences, tablet volume/route/processing controls, foreground-only
retirement and a separately consented mediaPlayback service are implemented.
[Current behavior](../audio.md) describes the settings and limits.

Permanent regressions exercise stereo ordering, malformed/truncated packets,
queue bounds, discontinuities, complete-frame/readiness/write deadlines, native
short writes, dead-track recreation, focus denial/pause/regain/permanent loss,
route changes, stale service notifications, direction independence and shutdown.
The normal native suite creates a private PipeWire server and runs both complete
CLI transport directions with fake authorized peers and ordinary native consumers.
It verifies sink enumeration, silent underflow, owned removal and server loss.

New adapter tests caught three boundaries before their fixes: incomplete native
blocks surviving a capture stall, SPA chunk metadata being ignored, and playback
teardown replacing the original failure with a locally closed socket error.
Local Stop now returns normally; actual remote resets and complete-frame timeouts
still fail. The tests remain in the normal suite. [Retained evidence](artifacts/2026-09-29-audio-speakers/README.md)
records the red/green results, counters and acceptance limits.

All 219 scoped Rust functions and 126 Kotlin methods pass the 80% line-coverage
gate; the full Android suite passes 701 tests. Connected-tablet checks established
native 48 kHz stereo playback, advancing frame counters, zero observed underruns
and owned Stop/removal with unchanged defaults. The native sink appeared in
233–518 ms and stopped in 15–16 ms in these bounded checks. These are startup and
retirement observations, not acoustic-delay measurements.

Audio does not change system defaults, move unrelated streams or automatically
route the microphone into the speaker sink. PipeWire dependencies are optional
for display startup. PulseAudio-only, Windows and macOS audio remain unsupported.
At this September 29 checkpoint, native device-clock correction and physical
duplex/AEC acceptance remained T720. The [September 30 integration](2026-09-30-audio-clock.md)
adds native counter correction; simultaneous physical duplex, effective AEC and
measured acoustic delay still require acceptance. Native frame counters and configured
buffer lengths alone do not establish acoustic latency or audible quality.
