# T445: attachment-only Activity launch

Periodic Android process recovery no longer issues Activity launches. A bounded
launch registry belongs to the monitor's observed attachments, independently of
forwarding readiness and connection epochs. Identity-confirmed aliases share a
launch allowance; a pending new-route probe can inherit the old route's history
before pruning. Confirmed missing routes immediately lose permission to launch,
including in-flight jobs. Unknown identities remain distinct. After 256 retained
route records, new routes require manual opening until pruning makes space.

The allowance is consumed synchronously after both reverse mappings succeed and
before the Activity command awaits. Failed forwarding can retry its initial
launch; route revalidation cannot repeat it. Explicit reopening remains possible.
A confirmed detach clears the allowance; a later attachment gets another attempt.
Token redelivery still uses the protected broadcast and cannot steal foreground
focus. T444 separately supplies attachment-specific credential rotation.

The permanent end-to-end fake-ADB regression failed before the fix: repairing
one route produced two Activity starts instead of one. Nine T445 tests now pass,
covering initial forwarding failure/retry, disabled auto-launch, repeated route
repair, process loss, explicit opening, identity-probed migration, fresh attach,
late jobs, serial reuse, synthetic devices, malformed identifiers and registry
capacity. The new registry's six production functions each have **100% of their
LLVM-counted executable lines covered** in the focused run. This does not certify
all existing monitor functions or the separate project-wide T497 target.

The daemon suite passed 374 tests with three existing ignored tests before the
last two focused launch cases were added; all nine focused cases then passed.
Clippy with warnings denied passed. Complexity: 4,075 functions, none above nine.
No physical tablet or EVDI attachment was used.

The accepted T445 policy intentionally supersedes T143's old expectation that a
missing process is relaunched. Its permanent scenario and retry-boundary checks
remain, asserting manual reopening instead. T390's independent-device progress
regression now stalls real token-delivery work, retaining its 250 ms independent
completion assertion. T110/T431 continue to cover token backoff and cancellation.

[Evidence](2026-09-19-attachment-launch/) retains the failing route test, focused
passes, broader daemon results, static checks and per-function line counts.
