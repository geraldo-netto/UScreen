# T652: tooling and bounded mutation campaigns

The maintainer selected **“Complete tooling and bounded campaigns”** instead of
executing every generated mutation. That scope is complete: the existing generated
Rust engine remains available, and a reviewed fault runner supports maintained
Rust, Kotlin, C and essential Python/Shell through explicit native build/test
commands. Both engines preserve baselines, private copies, source fingerprints,
diffs, logs and distinct outcomes. Neither reports a whole-project mutation score.

## Executed samples

Each campaign below has two distinct selected candidates, a passing unchanged
baseline, complete outcomes and restored private source bytes. All selected
production files still match this checkout. The same Python faults were also
executed on ordinary native Windows; these are additional platform executions,
not additional unique faults.

| Native sample | Baseline tests | Caught | Survived / unviable / timeout / tool error | Evidence |
| --- | ---: | ---: | --- | --- |
| Essential Python EDID, Linux | 3 | 2 | 0 / 0 / 0 / 0 | [Artifacts](artifacts/2026-09-27-bounded-mutations/python-linux/) |
| Essential Python EDID, Windows | 3 | 2 | 0 / 0 / 0 / 0 | [Artifacts](artifacts/2026-09-27-bounded-mutations/python-windows/) |
| Essential Shell EVDI setup, Linux | 4 | 2 | 0 / 0 / 0 / 0 | [Artifacts](artifacts/2026-09-27-bounded-mutations/shell-linux/) |
| Native C conversion, Linux | 8 | 2 | 0 / 0 / 0 / 0 | [Artifacts](artifacts/2026-09-27-bounded-mutations/capture-c-linux/) |
| Kotlin, JVM/Robolectric | 1 | 2 | 0 / 0 / 0 / 0 | [Artifacts](artifacts/2026-09-27-bounded-mutations/kotlin-jvm/) |
| Rust Linux supervisor `include!` | 1 | 2 | 0 / 0 / 0 / 0 | [Artifacts](artifacts/2026-09-27-bounded-mutations/linux-include/) |
| Rust native Windows tray/shared actions | 5 | 2 | 0 / 0 / 0 / 0 | [Artifacts](artifacts/2026-09-27-bounded-mutations/windows-tray/) |

The [catalog](../../scripts/mutation/catalog.json) retains exact edits and normal
regression selection. Review of the actual failed assertions established:

- PY01 produces an EDID checksum of 255 instead of zero. PY02 accepts physical
  width zero instead of raising `ValueError`; the permanent T321 tests catch both.
- SH01 adds existing devices instead of subtracting them, producing capacity 4
  instead of 2. SH02 admits zero and reaches setup instead of returning usage status
  2; permanent T269/T497 fixtures catch both without touching kernel devices.
- C01 shifts the dirty-row bit index; C02 changes a chroma coefficient. Both compile
  with the normal sanitizer/warning flags, then fail the independent T383 oracle's
  pixel/history/guard comparison. Compiler failures are not credited as kills.
- KT01 rejects `v1.2.4 > 1.2.3`. KT02 accepts overflowing `4294967296.0.0`.
  T123's permanent cross-language version fixtures report the exact mismatches
  in the retained fresh JUnit XML.
- RS01 reverses loopback rejection; RS02 rejects valid `inet`/`src` markers. The
  permanent T511 parser/bounded-invalid-input test fails its route assertion.
- WIN01 suppresses Quit, failing T531's shutdown-watch assertion. WIN02 reverses
  the stopping guard, failing the shared action/error-dispatch assertion (and
  native Settings delivery). These are test failures after successful builds,
  not process deadlines.

## Discovery and platform boundaries

[The scope listing](artifacts/2026-09-27-bounded-mutations/scope.json) uses the
maintained coverage inventory independently of cargo-mutants. It lists 238 files
containing maintained functions, plus source files with no maintained functions;
file hashes, all represented languages and unselected files remain visible.
The generated Rust discovery contains 4,991 candidates, retained in
[compressed raw JSON](artifacts/2026-09-27-bounded-mutations/cargo-inventory.json.gz).
Discovery does not mean execution.

Fifty-one maintained Rust files are reachable only through Linux `include!`
paths and have no normal engine candidates. The new runner directly edits the
selected `linux_main.rs` source span and compiles the original crate graph with
its actual native guards and normal tests. This proves a repeatable execution
route through the discovery gap; the remaining include descendants are explicitly
unselected. Native file reachability distinguishes Linux and Windows paths, and
mixed `cfg`/feature functions still need their own execution evidence. The one
Darwin scheduling file remains under declined T584. No platform exclusion or
coverage threshold was weakened to make mutation results pass.

The [earlier Rust campaigns](2026-09-27-mutation-progress.md) retain their original
scopes, equivalent survivors, unviable edits and semantic reviews. Their counts
must not be added to this sample to form a project score. Full Windows runs of
all shared-policy/video/command generated profiles remain unmeasured, outside the
accepted bounded scope. No physical tablet, input/display driver, GPU or camera
acceptance is established, and T382's declined performance campaign stays closed.

## Runner validation and isolation

Linux phases use Bubblewrap user/PID/network namespaces, a read-only host root,
a writable private output and private HOME/cache/tmp locations. Dependencies are
copied, not hardlinked; builds/tests run offline. Windows campaigns ran as ordinary
`BLENT-WIN11\blentdev` in session 1, using only the normal owned-resource fixtures.
Native process cleanup killed a child created in a separate process group. Removing
`taskkill /T` made that permanent regression fail; restoration made it pass. The
fixture's final cleanup uses its opened process handle, avoiding a late PID-only
kill. [Native negative control](artifacts/2026-09-27-bounded-mutations/windows-tray/descendant-red.log).

The normal runner suite passes 27 cases with its one Windows-only test explicitly
skipped on Linux, and 27 cases with two Linux-only tests explicitly skipped on
Windows. Thus both native platforms were exercised. Two additional permanent
scope tests verify guarded include traversal, shared descendants and cyclic graphs.
The maximum-complexity check passes every inspected function at nine or below;
[its retained output](artifacts/2026-09-27-bounded-mutations/complexity.log) records
the exact inventory. Python compilation and Rust formatting pass. Whitespace checks pass outside
retained `.diff` artifacts, whose blank context lines intentionally carry the
unified-diff space prefix. The new manual Linux workflow retains bounded artifacts; its GitHub
execution has not been dispatched in this local session.

Permanent normal-suite red/green regressions cover four tooling defects discovered
while completing this item:

1. A later `/tmp` mount hid an output directory placed under `/tmp`; mounting the
   owned output last fixes it. The retained negative control reruns the permanent
   namespace regression with the original ordering.
2. Inherited measurement hooks could write outside the sandbox or contaminate the
   independent line counters. Native phases now remove those hooks; the poisoned
   environment regression fails before removal and passes afterward.
3. Compiler stderr interrupted WIN01's Rust test-status line. The first campaign
   correctly retained `tool_error` instead of guessing a kill. The parser regression
   failed before adding final-failure-summary recognition; the corrected complete
   native rerun catches both faults. The
   [original raw classification](artifacts/2026-09-27-bounded-mutations/windows-parser-before-fix/report.json)
   is retained alongside the correction.
4. Diff output inherited the process text encoding. The retained Unicode/source
   restoration test failed under an ASCII locale; explicit UTF-8 diff output makes
   the same test pass, including the native Windows suite. CRLF source bytes are
   restored exactly, including interruption paths.

[Runner evidence](artifacts/2026-09-27-bounded-mutations/) retains red logs, passing
normal tests, per-campaign commands, source hashes, outcomes and checksums. Text
logs/JSON are normalized to UTF-8/LF for Git; diffs retain context whitespace.
Earlier snapshots can differ from later tool/documentation edits, as their
`checkout_matches_snapshot` fields state; each result describes its immutable
execution snapshot. Selected production-byte comparisons are retained separately.

T652 changes development tooling, tests and CI/documentation. Maintained application,
C and essential-script production behavior is unchanged by this commit. Their
independent native line gate remains required; the preceding T531 commit separately
passed all 59 changed-scope production functions at 80% or above. No newly collected
whole-project line or mutation result is claimed.
