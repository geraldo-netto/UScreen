# Blent follow-up implementation and validation

## T571 — replay completion

ACK obligations are enqueued before publishing render counts. Completion requires
an active session, every expected render and no outstanding ACK writes; reaching
the drain deadline alone is not success. The deadline remains 750 ms.

Permanent `UsbReplayTest` regressions hold the render-statistics monitor while
observing ACK publication, and exercise missing-render and undrained-ACK timeouts.
Both regressions fail against the original implementation on Android API 27 and
34; all six replay test executions pass after the fix. The tests were written
after implementation, as requested, then run against retained original source.
Existing restart/receipt coverage remains unchanged.

Raw [old-behavior failure](artifacts/2026-09-26-next-batch/t571-red.log.gz) and
[passing validation](artifacts/2026-09-26-next-batch/t571-green.log.gz) are retained.

## T576 — reproducible conversion replay builds

Both replay generators copy every C header from the selected revision, with the
conversion/frame-exchange sources unchanged. Conversion manifests hash every
copied file. Harness-only adapters handle generation-aware publication and use
named job fields, preserving pre-span historical baselines.

Permanent `scripts/tests/test_conversion_sources.py` clean-build tests cover the
working tree, HEAD and pre-header revision `d1de64a`, verify copied bytes/hashes,
and compare row/span replay checksums. Both tests pass; the same tests fail with
the original generators/harnesses. Retained [failure](artifacts/2026-09-26-next-batch/t576-red.log.gz)
and [passing logs](artifacts/2026-09-26-next-batch/t576-green.log.gz).

## T572 — native camera ownership and cleanup coverage

The normal Rust suite now creates fresh PTY character devices, binds fixture
labels over `/sys/dev/char` in a private child mount namespace, and runs the
unchanged native camera adapter with private fake ADB/FFmpeg executables.
It verifies successful device opening, exclusive locks, wrong labels, symlink
rejection, graceful stop, producer failure, reverse-mapping cleanup and release
of both output locks. No live webcam or real ADB mapping is accessed.

All 17 camera tests pass. `open_device` reaches **28/28 executable lines (100%)**;
`run_native` reaches **27/28 (96.43%)**. The remaining line logs a failed cleanup
attempt. [Counters](artifacts/2026-09-26-next-batch/t572-coverage.json),
[LCOV](artifacts/2026-09-26-next-batch/t572.lcov.gz),
[suite log](artifacts/2026-09-26-next-batch/t572.log.gz), and
[ordinary-user native run](artifacts/2026-09-26-next-batch/t572-user.log.gz) are retained.

The fixture requires working Linux user/mount namespaces. Ordinary-user execution
passes on the development host. Container CI grants SYS_ADMIN and disables its
mount-blocking AppArmor profile only for the regression container; namespace
setup failure fails the test explicitly. No production behavior was changed.
