# Raw slot offset assertions (T669)

The existing shared-layout test only checked that the final slot stayed within
the allocation. Replacing offset multiplication with addition or division still
satisfied that bound while producing overlapping or incorrect slot positions.

Permanent `t669_slot_offsets_partition_the_allocation_without_overlap` walks all
supported slot counts for four boundary/representative frame sizes. It checks
each exact contiguous offset, the final allocation boundary, and rejection of
out-of-range indices including `u32::MAX`. Production code is unchanged.

All six generated `Layout::slot_offset` mutations fail the new normal-suite test
after a passing baseline, with no survivors, build failures or timeouts. Fresh
LLVM counters pass all six raw-layout functions individually at 80% or higher;
the complete common library suite passes. This directly validates the shared
layout contract independently of native shared-memory consumers.

[Evidence](artifacts/2026-09-27-mutation-resume/t669/) retains the original
module outcomes, targeted regression run, source hashes, diffs and counters.
