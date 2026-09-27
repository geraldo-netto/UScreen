# T701: shared Android test-worker lifecycle

Robolectric reset the main looper while Compose's cached AndroidUiDispatcher
still considered a callback scheduled. A model-only test could enqueue snapshot
work between UI tests; resetting its queue left later UI work waiting for an
attachment callback that would not be scheduled again.

The permanent ComposeOrderTest reproduces the sequence explicitly:
DecoderDiagnosticsUiTest, ReleaseCheckTest's queued-preference-change case,
then SettingsAccessibilityTest's advanced-settings assertions. It runs on API27
and API34, checks that each requested method actually executes, and preserves
the original UI assertions. With the final test Application's draining disabled,
the final API27 regression still fails with AppNotIdleException at the unchanged
60-second deadline. With draining restored, the same regression passes.

A diagnostic read of the cached dispatcher showed scheduledTrampolineDispatch
true before draining and false afterward. Local cleanup in ReleaseCheckTest
fixed that narrow order but still failed the full suite. The shared Robolectric
test Application therefore drains queued main-loop tasks, including scheduled
frame work, during onTerminate, before the framework resets the looper. It does
not mutate dispatcher internals, change production Application behavior, skip
assertions, increase timeouts or fork one process per class. Existing Activity
sandbox configuration is retained. Test-only lifecycle code is not production
coverage scope.

The final instrumented testDebugUnitTest/profileCoverage run passes all 602
normal tests with zero failures, errors or skips in the shared worker. This
collection also includes the T707 camera retirement work, whose production
changes and scoped counters are recorded in its separate commit/review. It is a
whole Android unit-suite result, not a whole-project coverage or physical-device
acceptance claim. T417's separate decoder-UI scope decision remains unresolved.

[Evidence](artifacts/2026-09-28-compose-test-lifecycle/) retains the initial full
failure, permanent regression failure, dispatcher diagnostic, sampled thread
trace, final full result, JUnit results and exact source/fixture fingerprints.
