# Encoder marker validation (T665)

The x264 marker comparison mutation (`==` to `!=`) survived the original
encoder-worker tests. Existing invalid packets also lacked the exact worker
field, so they did not independently establish the marker requirement.

Permanent `t665_worker_fields_require_the_exact_x264_marker` accepts an intact
marker with a valid worker field, rejects two markerless packets containing that
field, and corrupts each marker byte independently. Production code is unchanged.
Both generated equality mutations in `effective_x264` fail this regression after
a passing unmodified baseline. The normal encoder-worker suite passes, and fresh
LLVM counters pass all three production functions individually at 80% or higher.

[Retained evidence](artifacts/2026-09-27-mutation-resume/t665/) includes source
hashes, the original survivor and baseline from an interrupted discovery campaign,
the complete targeted two-mutation rerun, failure logs, diffs and fresh counters.
The interrupted campaign is not a completed mutation result.
