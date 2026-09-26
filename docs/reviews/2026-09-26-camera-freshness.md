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

Whole-packet deadlines/reconnect and rate
adaptation are tracked separately in TODO.md until implemented. No native
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
