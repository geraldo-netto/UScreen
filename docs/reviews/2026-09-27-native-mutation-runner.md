# Native Windows mutation runner (T652, T666)

The Python runner executed under the ordinary, unelevated `blentdev` account.
Ten portable/native runner tests passed; the Linux-only process-group regression
was explicitly skipped. The Windows timeout test exercised native process
termination. This does not establish arbitrary detached descendant cleanup.

The first complete Python-driven autostart campaign had a passing baseline and
41 unique outcomes: 35 caught, two survived and four raw `Unviable` results.
Review found one of those four was infrastructure: Windows LNK1104 could not open
the generated test executable for `registry.rs:105:30` (`*` to `/` in `Key::write`).
That result is a build-tool failure, not an uncompilable source mutation or kill.

A fresh complete rerun with one worker passed its baseline and recorded
**36 caught, two survived, three unviable, zero timeouts**. Both source copies
remained byte-identical to their manifests. The previously interrupted link now
completed and its mutation failed the registry tests. An independent complete
PowerShell-driven campaign produced the same 36/2/3 results.

The surviving OR-to-XOR edits combine disjoint constants `KEY_QUERY_VALUE` (1)
and `KEY_SET_VALUE` (2), so both operators produce 3. The three source-invalid
edits require nonexistent `Default` implementations. Neither class is counted
as caught, and the runner's strict `passes` remains false.

[Retained evidence](artifacts/2026-09-27-mutation-resume/t666/) includes both raw
Python campaigns, the independent native campaign, baseline/failure logs, source
fingerprints, generated diffs, ordinary-user identity and runner test results.
The embedded Python 3.12 interpreter was private to the guest evidence directory.
Later cross-package runner extensions require their own validation.
