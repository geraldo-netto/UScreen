# Camera freshness — T614, T616, T617, T618

T614 implements BLCAM002, one packet in flight and a connection-local u64
acceptance counter. Feedback is sent only after the host writes the complete
packet to decoder stdin. It bounds transport admission; it does not certify
that FFmpeg decoded or a webcam consumer displayed the picture.

Sender logs preserve packet identity, feedback time and relative encoder-queue
age every 30 packets. Age compares PTS progression with sender monotonic time
since the first output; initial capture/encode delay is excluded. Camera2's
sensor clock is logged, not blindly subtracted from MediaCodec timestamps:
[Android documents compensation on direct encoder surfaces](https://developer.android.com/reference/android/hardware/camera2/CameraMetadata#SENSOR_INFO_TIMESTAMP_SOURCE_REALTIME).

The permanent real-H.264 upload regression failed before the change with
`T614 missing bounded camera feedback`, then passed. All 19 host camera tests
and the Android camera suite pass. JaCoCo measured all 34 functions in the
changed Android files ≥80%; LLVM measured all six functions in decoder/protocol
≥80%. Complexity: 5,825 functions, none above nine. [Evidence](artifacts/2026-09-26-camera/freshness/).

Rate adaptation is tracked separately in TODO.md until implemented. No native
performance improvement or absolute camera-to-display latency is claimed here.

## T616 — stale encoded-frame recovery

A permanent Camera2/MediaCodec adapter regression first observed seven packets
where only four were safe. It now observes four: configuration, initial keyframe,
fresh recovery keyframe and its dependent frame. Three stale/dependent frames
are dropped, exactly one sync request is issued, and every dequeued buffer is
released. Pure policy tests cover the exact freshness boundary, initial keyframe
wait, missing keyframe timeout and invalid budgets. Initial budget is 150 ms,
configurable 50–2000 ms through the portable profile, invitation, CLI and GUI.

Android changed-file coverage: 35/35 functions ≥80%. Rust profile, bridge and
camera settings: 20/20 functions ≥80%, including the full GUI suite. Camera host
suite remains 19/19 passing. Complexity: 5,832 functions, none above nine.

## T617 — whole-packet deadlines and generation retirement

The missing-feedback regression exceeded its 500 ms test bound before the fix;
with a configured 50 ms budget it now terminates and rejects reuse. A separate
real-socket regression blocks a 2 MiB packet mid-write, verifies only a prefix
arrived and ensures the retired connection cannot accept a new packet. Okio's
socket deadlines share one absolute deadline across write and feedback phases;
its normal 8 KiB staging is preserved. Host decoder input and ACK writes use the
configured freshness budget too.

Recovery is bounded to two retries with 250/500 ms backoff inside the original
consent owner. Tests prove resources retire before the next attempt, final
failure stops, cancellation during I/O/backoff prevents reopening, and unrelated
camera errors do not retry. Camera adapter tests exercise the production recovery
entry point. All 58 functions in changed Android files and all four host decoder
functions meet ≥80% coverage. All 19 host camera tests pass; complexity gate
reports 5,841 functions, none above nine.
