# Requested follow-up implementation

T418 shared input and its matched latency measurements are recorded in the
[dedicated report](../benchmarks/2026-09-21-shared-capture/README.md). Both
applications were subsequently reinstalled/reloaded and live FFmpeg 6.1.6
VAAPI output verified. T222 is deferred by the maintainer and no longer blocks
ordinary authorized reloads.

## T566 — report configuration changes after persistence

`storage::write_at` retains the semantic diff before writing but logs it only
after temporary-file write, synchronization and atomic replacement succeed.
Unchanged saves remain quiet. The new permanent Linux failure regression reads
an existing configuration through procfs, where creating a replacement is
forbidden even as root; it failed with the premature success message before
the fix, then passed after it. A portable success/no-op regression verifies
the saved value and qualified diagnostic behavior.

Validation: 91 common tests and both existing host diagnostic regressions
passed; the changed method measured 18/20 executable lines (90%). Complexity
remains at most 9. [Evidence](2026-09-21-follow-up-evidence/t566.tar.gz) retains
red/green logs, source manifest and the scoped coverage report. Other methods
in that common-only report are outside this change's measurement scope; it is
not a replacement for the full-platform coverage gate.
