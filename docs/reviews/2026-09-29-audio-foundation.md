# T717: shared audio foundation

Completed the portable shared layer for the accepted selectable-device contract.
`common/src/audio.rs` and its modules provide profiles, truthful capability gates,
backend interfaces, per-direction session ownership, credential-bound framing,
bounded PCM queues and drift resampling. [The implemented contract](../audio.md)
documents exact wire fields, integration obligations and evidence limits.

Each explicit Start issues a fresh credential through the existing stock-getrandom
helper. That helper is now available with the common crate's minimal feature set;
no credential is persisted and no new dependency version was added. Stop closes
admission, clears audio and waits for matching native retirement before replacement.
Unknown capabilities fail closed. Native Linux/Android adapters and GUI controls
are still T718/T719; this change does not open microphones or publish audio devices.

The accepted format is fixed 48 kHz s16le, mono microphone/stereo speakers, 480-frame
blocks. Handshakes bind version, credential, generation, direction, channels, rate,
processing, background consent and queue preference before payload admission.
Fixed headers are validated before bounded payload reads; complete frames reject
truncation, trailing data, replay, reordering and generation/direction changes.
Sequence gaps report discontinuity and flush queued audio. IO implementation remains
an adapter responsibility, with explicit cancellation/deadline requirements.

The configurable 20–200 ms queue target defaults to 40 ms. Fixed 200 ms capacity and
local age expiry bound queued PCM; overflow drops oldest blocks and underflow emits
silence. Fixed-point interpolation preserves stereo channels across block boundaries.
Native counter ratios drive a bounded ±1000 ppm correction; excess drift flushes.
These are application policy bounds, not measured acoustic latency or quality.

## Permanent tests and validation

All tests run in the normal Rust suite; no tests were removed, skipped or weakened.
The 26 T717 tests include fake native ownership, injected monotonic clocks, independent
directions, rapid replacement and stale callback cancellation, reconnect requiring
fresh Start, authentication, a wire-layout fixture for future Android integration,
every handshake-byte mutation, packet truncation and malformed sizes, signed sample
extremes, sequence gaps/wrap, bounded mutations, queue overload, underflow, expiry,
configurable prefill and drift/interpolation boundaries. The bounded queue stress
case performs 4,096 operations; the frame mutation corpus has 2,048 cases.

During construction, permanent tests exposed and then verified fixes for:

- `t717_stale_render_cannot_retire_replacement`: a stale renderer called deadline
  polling before validating its generation. Ownership validation now happens first.
- `t717_handshake_cannot_move_clock_backwards`: connection admission lacked the
  session's local starting-clock lower bound. Handshake and frame admission now
  reject clock regression.
- `t717_poll_clock_must_not_regress_before_frame_arrives`: successful polling did
  not advance the last observed clock. Later frame arrival could move time backwards.
- `t717_fractional_excess_drift_is_rejected_before_rounding`: rounding the ratio
  first admitted a fractional excess beyond ±1000 ppm. The exact ratio is checked
  before truncating to integer ppm.

[Retained evidence](artifacts/2026-09-29-audio-foundation/) includes red logs,
passing normal-suite logs, fresh LLVM counters, the pre-collection source manifest,
per-function report and whole-project complexity results:

- Minimal-feature common crate: **109 tests pass**.
- Default-feature common crate: **26 audio tests pass**.
- Scoped native Linux LLVM gate: **42/42 functions at 100% executable-line coverage**,
  covering all 39 authored audio functions and the three shared credential functions.
- Whole-project source complexity: **6,970 functions, none above 9**.
- Linux `cargo check --locked --workspace`, Rust formatter and diff checks pass.

Coverage used a new target directory after the last production change, with matching
source fingerprints. This is a scoped result, not a new whole-project coverage claim.
Shared Rust behavior was tested on Linux; no native Windows, Android audio, PipeWire
node, acoustic AEC, microphone permission or end-to-end latency acceptance is claimed.
T718 is now implementable; T719 follows T718, and T720 retains native duplex acceptance.
