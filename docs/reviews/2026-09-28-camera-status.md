# T704: expire stalled camera status

The host frame monitor now expires Streaming after two seconds without another
selected decoded frame. It then reports Waiting and clears the preview. The
output writer and status monitor share the same two-second freshness constant;
the GUI's existing independent preview freshness check is preserved. Fresh
selected frames renew the deadline and recover Streaming. Empty frames cancel
it, and unselected frames do not extend it.

Both permanent controlled-clock regressions in `camera/status_tests.rs` failed
before the fix with Streaming instead of Waiting. They now cover 1,999/2,000 ms,
renewal, recovery, unselected frames, empty frames, cancellation/Stop and a new
rear-lens monitor after the front monitor is retired. Existing camera/controller
lifecycle tests remain in the normal suite. No tablet or webcam device is needed
for these state tests; native camera acceptance remains separately tracked.

Validation: 22 camera tests and all 206 host library tests passed. A fresh
LLVM coverage target measured all 15 functions in camera.rs and camera/outputs.rs
above 80% individually; frame_status is 32/33 executable lines (96.97%).
Complexity and formatting checks passed. This is scoped coverage, not a new
whole-project coverage claim. The source snapshot was refreshed after the
unrelated Android order-test probe changed; all measured Rust sources were
unchanged throughout collection.

[Evidence](artifacts/2026-09-28-camera-status/) includes red/green results, raw
LCOV, the source manifest, per-function report and complexity result.
