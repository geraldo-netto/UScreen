# T720: native audio clock integration

The clocked audio protocol carries the sender's native frame position, epoch and
monotonic timestamp. Shared Rust policy and equivalent Android policy compare
independent native rate windows, then apply bounded linear resampling at the
receiving endpoint. Absolute device clocks and network arrival rates are never
subtracted or treated as equivalent clocks. [Current contract](../audio.md).

PipeWire's [SPA driver clock](https://docs.pipewire.org/structspa__io__clock.html)
is read through a size-checked native adapter. Clock ownership changes, driver
identity/rate changes and frame/time resets invalidate earlier rate evidence.
Android uses [AudioRecord.getTimestamp](https://developer.android.com/reference/android/media/AudioRecord#getTimestamp(android.media.AudioTimestamp,%20int))
with TIMEBASE_MONOTONIC and [AudioTrack.getTimestamp](https://developer.android.com/reference/android/media/AudioTrack#getTimestamp(android.media.AudioTimestamp)).
Unavailable counters remain unmeasured, and playback focus flushes/recreation
start a new epoch. Native API timestamps do not prove physical presentation or
capture time at an acoustic boundary.

The fixed 24-byte v2 prefix is validated atomically before receiver state changes.
The legacy v1 wire remains supported by the host and retains its existing tests.
Queue/prefill bounds remain 20–200 ms, with a twenty-block cap and 200 ms age limit.
Counter windows span two to five seconds; completed observations expire after
three local seconds. Correction is bounded to exactly ±1000 ppm, including
fractional over-limit ratios before rounding. Missing, stalled, reset or excessive
counters cannot fabricate a valid measurement. Silence and re-priming preserve
existing recovery behavior.

Permanent T720 tests exercise fixed encodings and bounded truncations, invalid
and out-of-range counters, atomic rejection, duplicate/stale snapshots, clock
offsets, reset/epoch changes, exact drift limits, stereo interpolation, focus
and dead-track ownership. Robolectric tests execute Android adapter methods on
API27 and API34. Private PipeWire tests execute both complete host CLI directions,
legacy and clocked transports, real advancing graph counters, synthetic PCM,
underflow, server loss and owned retirement. They create no physical audio route.

Physical simultaneous microphone/speaker use, intentional speech/AEC comparison,
alternate routes, USB loss/reconnect and measured acoustic delay still need an
identified route and a maintainer device window. No speech recording, install,
system default, volume, application route or physical-device test was performed
for this integration. T720 retains only that missing acceptance; native AEC being
enabled is not evidence of effective echo cancellation.

[Validation artifacts](artifacts/2026-09-30-T720/README.md) retain measured scope
and command outcomes. Earlier audio reviews describe their dated checkpoints.
