# T706: camera service ownership

Each background-service lifetime has a fresh UUID, carried by its start Intent.
The service remembers the current accepted run, and teardown can retire only
that run. Stop/restart and process-owner reset cannot reuse its identity.
Replacing a lens while the same background service remains active retains that
service's ownership. A reused service accepts the new start; stale or malformed
intents cannot displace a current owner. Failed foreground promotion retires
only an accepted delivered run, without guessing ownership during `onCreate`.

The permanent `CameraServiceOwnershipTest` reproduced old-service destruction
clearing the replacement's REAR selection before the fix. Its same selection
and endpoint assertions now pass on API27/API34, alongside current-service death,
reused-service, stale/null/empty identity, promotion-failure and owner-reset checks.
Existing permission, Activity and capture contract assertions are retained.
Promotion failure is injected at the service readiness boundary; no physical
camera or Android foreground-service scheduling acceptance is claimed.

The instrumented run passed CameraContractTest, CameraHostControlTest and
CameraServiceOwnershipTest on both SDKs, plus the then-current T701 order probe.
All 28 production functions in CameraBinding.kt, CameraOwner.kt and CameraService.kt
have native JaCoCo counters at or above 80% individually. Complexity check passed.
These are scoped results; T701 continues to track the combined Android worker.

[Evidence](artifacts/2026-09-28-camera-service-ownership/) retains the failing
assertion, passing run, source manifest, raw JaCoCo counters, per-function report
and complexity result. Source/test hashes correspond to collection, before
later narrowing of the unrelated T701 test-order probe.
