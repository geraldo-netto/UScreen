# Mutation testing (T652)

The native Rust profiles run production-source mutations against permanent normal
tests. They complement the independent [80% per-function line gate](../coverage/README.md).
No whole-project mutation score or agreed score threshold exists. A profile passes
only with a passing unmodified baseline, a nonempty unique inventory, complete
outcomes, and every candidate caught. Survivors, unviable edits, timeouts and tool
errors remain distinct and make this strict campaign check fail.

```sh
cargo install cargo-mutants --version 27.1.0 --locked
python3 -m unittest discover -s scripts/tests -p test_mutation_runner.py
python3 scripts/mutation/run.py --profile usb --output /absolute/new/evidence
```

Use `linux-autostart` on Linux or `windows-autostart` on native Windows.
`usb` supports both platforms; each execution records its actual platform.
Windows execution must use an ordinary account. Tests use isolated registry keys,
fake ADB processes and owned sockets; no physical device acceptance is implied.
The Python orchestration command is validated on Linux. The retained Windows
campaigns use the same native cargo-mutants selection through a PowerShell
driver; native validation of the Python timeout/process adapter is still pending.

Defaults: two workers, six compiler jobs, 60-second test timeout, 600-second build
timeout and a two-hour campaign deadline. Adjust `--jobs`, `--build-jobs`,
`--test-timeout`, `--build-timeout`, `--campaign-timeout` for available capacity.
More workers use more memory/disk. A timeout is missing evidence, never a kill.
Use a new output directory outside the checkout for every run. The runner copies
tracked and nonignored working files, records exact SHA-256 hashes and command,
and lets cargo-mutants work in further private copies. It never mutates the live
checkout. Ignored build trees, local signing material and device settings are not
copied. Logs, diffs, candidate inventory and outcomes remain under `mutants.out`.
Campaign process trees have bounded lifetimes, including timeout cleanup.

The manually dispatched `mutation.yml` workflow retains the same evidence.
It does not replace existing line-coverage CI. Changes to the private snapshot
invalidate its report. Working-checkout changes are reported separately; evidence
always describes the fingerprinted snapshot, not later edits.

## Scope and interpretation

`profiles.json` explicitly selects files and normal-suite test targets. Rust's
normal discovery does not expand `include!`; Linux supervisor files and their
descendants therefore need separate discovery/execution before claiming coverage.
The retained [initial inventory](../../docs/reviews/2026-09-27-mutation-testing-handoff.md)
also lists foreign-platform candidates. Never credit Linux execution for Windows
code disabled by `cfg`. A file with no generated candidates is not a tested file.
Macro bodies and operators unsupported by cargo-mutants are not exhaustive fault
coverage. Android, C and essential Python/Shell campaigns remain pending T652.

For each survivor, first verify its selected tests include existing integration
coverage. Then classify equivalent changes with a concrete semantic explanation
or record a TODO for missing assertions/behavioral defects. Keep new regressions
in the normal suite: fail against the mutation, pass against original production
code. Real behavioral fixes additionally require the maintainer's test-before-fix
workflow. Never weaken assertions, mutate tests, hide survivors or count build
failures as caught faults.

Review caught results too: T664 found a test-fixture failure reported as caught
for a semantically identical edit. The JSON outcome is the engine's raw result,
not proof that the failure was caused by the mutation. Retain raw evidence,
classify unrelated failures explicitly and rerun after the fixture is corrected.

Engine references: [configuration](https://mutants.rs/config-file.html),
[exit codes](https://mutants.rs/exit-codes.html). The importer uses phase outcomes,
not the process exit code alone; missing/incomplete evidence fails closed.
