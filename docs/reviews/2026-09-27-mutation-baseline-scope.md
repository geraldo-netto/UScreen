# Match mutation and baseline test scope (T671)

Cargo-mutants 27.1.0 applies `--test-package` to mutant runs but derives baseline
packages from the mutated sources. A shared-video experiment therefore tested
only `blent-config` before running mutations through both common and host tests.
The old importer checked baseline success but did not compare its test command
with mutant commands. A smaller baseline cannot establish that caller tests pass
without the mutation.

The runner explicitly forwards consumer packages as Cargo arguments for both
phases. The importer independently compares each recorded phase command against
the baseline and rejects missing or different commands. Package ordering,
duplicate selectors and the engine's version-qualified selectors normalize to
the same package set; feature, target and test-filter differences remain visible.
Raw outcomes remain intact, with a separate `baseline_matches_mutants` result.

Two new permanent regressions failed before the fix. All 14 runner tests pass
on Linux; native Windows passes 13 with the Linux-only descendant test explicitly
skipped. A real six-candidate Rust Android-command campaign now runs its 18 host
consumer tests in the unmodified baseline and catches all six mutations, with
matching commands and no infrastructure failures. This does not measure Kotlin.
The final importer also reprocessed the retained before/after engine artifacts,
rejecting the mismatched earlier run and accepting the corrected one.

Whole-project complexity passes: 6,293 functions, none above nine. The runner is
a nonessential development tool under the existing line-coverage exemption;
its permanent behavior regressions remain in the normal script suite.

[Evidence](artifacts/2026-09-27-mutation-resume/t671/) retains red/green tests,
native identity and hashes, the earlier mismatched campaign, corrected engine
commands, all raw outcomes/logs/diffs, and final-importer reports.
