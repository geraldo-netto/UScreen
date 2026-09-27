# Decoder negotiation assertion gaps (T668)

Permanent shared-policy tests now independently validate report dimensions and
frame-rate bounds, codec-family limits and hardware subsets, both forbidden
legacy extensions, invalid reports/streams, hardware ordering, and supported
profile/depth/level combinations. Production negotiation behavior is unchanged.

The original module campaign left 18 negotiation mutations alive. The complete
rerun passes its unmodified baseline and catches **97 of 99** mutations, with
zero survivors or timeouts. The two remaining candidates cannot compile because
`DecoderChoice` has no `Default` implementation; they are not counted as caught.
The 13 normal negotiation tests pass, as do all 127 common library tests.

Fresh LLVM counters pass all 18 maintained negotiation functions individually
at the existing 80% threshold. [Retained evidence](artifacts/2026-09-27-mutation-resume/t668/)
contains baseline and mutant logs, original/final outcomes, exact source hashes,
diffs and scoped counters. These are shared Rust policy results; Android decoder
runtime and native device acceptance remain separate.
