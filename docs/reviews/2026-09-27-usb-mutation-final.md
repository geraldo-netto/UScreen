# USB campaign verification and equivalent mutation (T664)

The final native Linux and Windows campaigns each execute all 125 selected
candidates after a passing unmodified baseline: 112 caught, 12 compilation-invalid
and one survivor, with no timeouts. Their production USB sources and permanent
USB tests match the current files. Full-checkout fingerprints can differ because
other documented work continued after the isolated snapshots were taken.

The survivor replaces `CaptureResources::default()` with `Default::default()` in
`Unavailable::resources`. The declared return type remains `CaptureResources`, so
both call the identical `Default` implementation. It is equivalent, retained as
survived, and never added to the caught count. Compilation failures also retain
their distinct status. The strict all-candidates-caught runner reports false.

The pre-reboot Linux run incorrectly appeared to catch that same equivalent edit.
Its retained log shows `t655_concurrent_listener_fixtures_reserve_disjoint_pairs`
panicking with `AddrInUse`; the snapshot predates the final serialized fixture.
That is an unrelated test-fixture failure, not mutation sensitivity. The fresh
complete run uses the final fixture and reproduces Windows' equivalent survivor.
The old raw report remains historical evidence with this correction.

[Retained evidence](artifacts/2026-09-27-mutation-resume/t664/) includes the
invalidated old log, complete fresh Linux outcomes/inventory/baseline/logs/diffs,
source manifest and full native Windows evidence. This closes verification of
the selected USB campaign; it does not establish a whole-project mutation score.
