# Idle-policy assertion boundaries (T667)

Permanent normal-suite tests now check every baseline/trial admission step,
the inclusive 100 ms ACK bound, the exact one-second transition grace,
the 1.5-second sparse timeout, and both sides of the 20 ms cost margin.
A descending cost window establishes the actual percentile used for admission;
ACK limits are tested independently of relative cost. Production code is unchanged.

The original module campaign left 18 idle mutations alive. With the new tests,
a complete campaign passes its baseline and catches **84 of 86** mutations,
with no unviable edits or timeouts. The two surviving subtraction-to-addition
edits in `observe` are equivalent: one adds twice the fixed reference ready time
to every cost; the other subtracts twice the fixed reference presentation time.
Each shifts the percentile baseline by the same constant, which cancels in every
cost-minus-baseline comparison. Reference resets discard the old cost window.
Bounded input timestamps cannot overflow the i128 intermediate calculations.
These remain raw survivors, not credited as caught mutations.

The 12 normal idle tests pass. Fresh native LLVM counters pass all 13 production
functions individually at the existing 80% gate. The full common library suite
also passes. [Evidence](artifacts/2026-09-27-mutation-resume/t667/) retains the
original and final outcomes, baselines, mutation failure logs, diffs, source
fingerprints and scoped counters. Native capture/device acceptance is separate.
