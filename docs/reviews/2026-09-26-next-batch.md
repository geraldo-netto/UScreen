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
