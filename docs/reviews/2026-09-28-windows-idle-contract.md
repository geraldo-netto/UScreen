# T695 portable cadence control

`common::idle::control` supplies an owned cadence interface and a bounded
controller for future capture adapters. `Control` binds evidence to nonzero
encoder, named decoder and viewer epochs. It keeps at most 256 encoded timings,
matches each ACK ordinal/sequence to its encoded media timestamp and host-ready
time, and delegates baseline/trial/admission to the existing T492 policy.
Missing timestamps, gaps, stale clients, epoch changes and adapter errors restore
compatible cadence. Dropping the controller clears its request. A new session
must establish new evidence; no persisted certificate crosses an epoch.

The adapter contract applies sparse requests only to unchanged frames, preserves
normal motion and periodic keyframes, expires requests after 1.5 seconds on a
monotonic clock, and clears only its owned request. Linux's existing FIFO lease
now implements this interface without changing its inode/token ownership or
capture loop. The portable controller is available for Windows stream integration;
it does not replace Linux's existing tracker integration in this change.

The permanent Windows fixture implements the contract with an owned Win32 event
and bounded state. It exercises native event delivery, consumption, request
expiry, motion bypass, invalid cadence and teardown. Shared tests cover complete
baseline/trial/active transitions, timing/ordinal bounds, encoder/decoder/viewer
changes, missing evidence, adapter failure and cleanup. Existing Linux FIFO,
stale-ACK, motion and keyframe regressions remain in the normal suite.

This is integration preparation. No Windows capture loop or in-process encoder
has been activated, and the event fixture is not a shipped capture backend.
T528/T529 and native functional acceptance still gate production wiring. No
battery, latency or CPU benefit is inferred from these tests.
