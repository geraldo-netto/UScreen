# T707: camera retirement off the Activity thread

Stop closes resource admission, interrupts transport and cancels capture without
performing native destruction on the Activity caller. Capture's non-cancellable
IO cleanup owns destruction after its borrowed native operations return. A
separate retirement chain waits for both the previous worker and earlier
retirements, so cancelling an intermediate replacement cannot bypass an older
capture. Workers cancelled before starting also retain their resource cleanup.
UI selection/service state retires promptly; a service-stop exception cannot
skip the native cleanup in the enclosing finally block.

Camera2 startup has an explicit outstanding-callback lease. A callback transfers
its device/session to the resource owner before releasing that lease, including
callbacks arriving after cancellation. Native close therefore occurs on the
retirement worker, once, rather than in the main-thread callback. Synchronous
startup failures release the lease; duplicate terminal callbacks cannot retire
an owned object twice. Concurrent closers wait for the same destruction.
Native failures retain their cause and cannot become transport retries merely
because cancellation interrupted the socket. Retry, permission and service-run
ownership contracts remain covered.

The permanent CameraRetirementTest first failed on both API27/API34 because
native destruction ran on the Activity caller and resources were destroyed
during an outstanding capture operation. The same assertions now pass, with
blocked destruction, late startup and rapid replacement checks. A separately
retained failing regression caught a service-stop exception bypassing cleanup;
that path now passes too. CameraStartupTest covers late callbacks, duplicate
release/error delivery and concurrent close. CameraDeadlineTest verifies Stop
interrupts an actual local socket handshake before native cleanup or the normal
handshake timeout. Existing native-adapter and lifecycle assertions remain;
tests that asserted synchronous cleanup now wait for the same expected result.

The final normal instrumented Android suite passes 604 tests with no failures,
errors or skips. Fresh JaCoCo counters pass all 65 production functions in
CameraBinding, CameraCapture, CameraRecovery, CameraResources, CameraStartup and
CameraWire individually at 80% or more. Complexity and whitespace checks pass.
These are scoped counters and automated lifecycle results, not a whole-project
coverage claim or physical Camera2/codec scheduling acceptance.

A native call or outstanding callback that never completes keeps retirement
pending: Stop and the UI remain responsive, while a replacement waits rather
than destroying or reusing a live native resource. No native-destruction duration
is guaranteed. Physical camera/consumer acceptance remains T621, including these
lifecycle transitions; physical cable removal remains T540. No device camera,
Windows backend or live desktop was used for this work.

[Evidence](artifacts/2026-09-28-camera-retirement/) retains red/green results,
JUnit camera results, the source snapshot, raw JaCoCo counters, per-function
coverage and complexity output. T701 separately fixes the test-worker lifecycle;
its shared Application cleanup is active in this final collection.
