# Mutation testing checkpoint — T652

Historical initial checkpoint. See the [post-reboot progress](2026-09-27-mutation-progress.md)
for completed campaigns, fixes and current remaining work. Statements below describe
the state at the original pause.

Paused at the maintainer's request to save progress for a later session.
Resume from this document and the open T652 row in `TODO.md`. No mutation
campaign is still running. This checkpoint is not completion of T652.

## Request and established coverage

The maintainer asked whether coverage is at least 80% per function/method and
requested review plus mutation tests. The answer is yes for the existing agreed
scope: the [last combined gate](artifacts/2026-09-27-final-review/summary.json)
passes 2,293/2,293 functions/methods individually, with no aggregate substitution.
Rust Linux/Windows, Android, C EVDI and essential Python/Shell are included;
the existing Darwin disposition and benchmark/development-tool exemptions remain
unchanged. Production code has not changed since that measurement.

Mutation testing measures assertion sensitivity to deliberate faults; line
coverage alone does not establish that sensitivity. No mutation-score threshold
has been requested or agreed. An optional scope question was sent: whole
maintained project versus recent USB/autostart changes first. It was unanswered
when the maintainer asked to save. Do not treat silence as agreement to a narrower
scope or claim a whole-project mutation result from this initial parser campaign.

## Completed investigation and experiment

- Repository review found no mutation runner or mutation CI configuration.
  Existing input-data corruption tests are useful but are not production-source
  mutation campaigns. T652 records the required work before implementation.
- Installed `cargo-mutants` is 27.1.0. An unfiltered workspace inventory found
  4,908 candidates across 111 files. This is an inventory, not executed tests.
- Ordinary discovery omits the Linux supervisor reached through `include!` and
  other maintained files; it also lists foreign-platform code. The exact list
  of maintained Rust files without candidates is retained in the summary.
  Some candidate-free files may simply have no applicable mutation operator;
  classify them individually. Do not count Linux execution of cfg-disabled
  Windows code as meaningful native mutation validation.
- A Linux parser campaign executed 26 mutations in `with_synthetic`, `known_state`,
  `record` and `parse`. Its unmodified baseline passed and **all 26 were caught**,
  with zero survivors, timeouts or unviable mutations. It uses existing permanent
  tests selected by `adb_inventory::tests`. `query` and `package_presence` were
  explicitly outside this small first campaign.
- The first attempt from the main checkout stopped before its baseline because
  copying encountered an unreadable ignored FFmpeg build directory. No results
  from that attempt count. Repeating from a clean detached worktree succeeded.
- No production behavior, permanent tests, mutation runner, dependencies or CI
  configuration has been changed yet. There are no discovered surviving mutants
  to fix from the completed parser campaign.

The [checkpoint artifacts](artifacts/2026-09-27-mutation-checkpoint/README.md)
retain source fingerprints, inventory, exact generated mutations, outcomes,
baseline/test logs and diffs. Production source commit: `c20717a`.

## Reproduce the completed parser campaign

Use a clean detached worktree to avoid reading existing build/cache trees. Each
run needs its own output directory. From that worktree:

```sh
TMPDIR=/path/to/private/scratch CARGO_BUILD_JOBS=6 cargo mutants \
  --package blent --file host/src/adb_inventory.rs \
  --exclude-re 'query|package_presence' \
  --cargo-arg=--lib --cargo-arg=--locked \
  --cargo-test-arg=adb_inventory::tests \
  --timeout 20 --build-timeout 600 \
  --copy-target=false --jobs 2 --jobserver-tasks 6 \
  --output /path/to/fresh/evidence
```

Baseline testing remains enabled. Never infer a killed mutant from compilation
failure, timeout or failed infrastructure. Preserve these as distinct outcomes.

## Next work

1. Resume the scope discussion if needed; review applicable mutation operators,
   normal-suite test selection and native execution for every selected language.
   Maintain explicit tested/unmeasured scope. Keep the independent 80% line gate.
2. Add a repeatable, bounded mutation command and retained configuration/CI entry.
   Use isolated copies; never mutate the user's live checkout, device state or
   test assertions. Require an unmodified passing baseline, source fingerprints,
   unique source edits, separate outcome classes, bounded processes and logs.
   Handle `include!` discovery gaps explicitly instead of silently excluding them.
3. Run campaigns and review meaningful survivors. Record each new finding in
   TODO before fixing it. Strengthen permanent normal-suite tests, demonstrate
   failure against the mutant and success against original code. If an actual
   behavioral bug is discovered, use the repository's test-before-fix process.
   Do not weaken tests or hide survivors/unmeasured code to improve a score.
4. Validate runner failure handling, normal regressions, whole-project complexity
   (maximum 9), formatting and appropriate native per-function coverage. Commit
   resolved findings individually; remove T652 only when its agreed work is done.

A possible approach considered, **not implemented or selected**, was a bounded
catalog of explicit fault mutations spanning Rust, Kotlin, C, Python and Shell,
using native normal-suite tests with separate build/test phases, plus broader
Rust campaigns with `cargo-mutants`. Such a catalog must be labeled as curated
coverage, not exhaustive per-method mutation testing. Tool alternatives examined:
[cargo-mutants configuration](https://mutants.rs/config-file.html),
[baseline handling](https://mutants.rs/baseline.html),
[timeouts](https://mutants.rs/timeouts.html),
[mutmut](https://mutmut.readthedocs.io/en/latest/),
[Android PIT plugin](https://github.com/koral--/gradle-pitest-plugin), and
[Mull](https://mull.readthedocs.io/en/latest/GettingStarted.html).
Only cargo-mutants has been run. No PIT, Mull or mutmut integration was validated.

## Local resume resources

- Main branch: `configurable-input-devices`.
- Previous batch was pushed through `c20717a`; this checkpoint has not been pushed.
- Raw experiment root: `/backups/disk2/blent-mutation-evidence`.
- Retained clean worktree: `/backups/disk2/blent-mutation-evidence/parser-worktree`,
  detached at `c20717a`; no need to create it again.
- Tool environment: `/tmp/blent-batch-venv/bin/python` provides existing coverage
  and complexity dependencies. Recreate from repository requirements if missing.
  Do not prepend that venv to PATH for full Python tests: system Python has Xlib.
- Full Linux tooling requires `rpmbuild`; the prior private shim is
  `/tmp/blent-batch-rpm/bin`. Missing external tools are failed validation, not
  caught mutants. Normal all-features FFmpeg headers used the private
  `/tmp/blent-batch-t612/ffmpeg-dev/usr/lib/x86_64-linux-gnu/pkgconfig` path.
- Native Windows VM and credentials/preferences remain unchanged. Preserve its
  disk, TPM and installation marker. Native helper scripts were copied from
  `/tmp` into `/home/netto/.local/share/blent-dev/mutation-checkpoint` for resume.
  Existing VM instructions and prior coverage commands remain in the private
  `/home/netto/.local/share/blent-dev/implementation-batch-handoff.md`.
- Guest `C:\src\blent` has the synchronized T651 sources over its older Git HEAD.
  Do not reset/pull over those synchronized changes. Native mutation runs must
  use isolated guest source copies and ordinary-user execution.
- No physical tablet, display or camera acceptance is authorized by this pause.
  Preserve existing blocked/deferred/declined dispositions, especially T382/T584.

Do not push the checkpoint or future mutation changes without a new push request.
The earlier push authorization was exercised for the completed implementation batch.
