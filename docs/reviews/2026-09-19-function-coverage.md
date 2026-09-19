# Per-function coverage and retained tests — 2026-09-19

T497 establishes a minimum of **80% executable-line coverage for each production
function/method**, rather than an aggregate project percentage. The Linux,
Android, C and essential-script scopes pass. Native Windows coverage remains
unavailable locally and is explicitly unresolved under T493/T497.

## Measured scope

| Scope | Functions/methods meeting the threshold | Below threshold | Unmeasured |
| --- | ---: | ---: | ---: |
| Linux Rust applications, default + all features | 1,021 / 1,021 | 0 | 0 |
| Android main app | 490 / 490 | 0 | 0 |
| C EVDI capture helper | 106 / 106 | 0 | 0 |
| Essential Python installation/packaging scripts | 48 / 48 | 0 | 0 |
| Essential shell installation/EVDI/packaging scripts | 55 / 55 | 0 | 0 |
| Windows-only Rust functions | 0 / 58 | — | 58 |

These are threshold pass counts, **not 100% line coverage**. Rust combines normal
workspace runs with and without the optional in-process encoder. The final Linux
report excludes only explicitly Windows-guarded functions/modules and records
that scope; the accompanying all-platform report retains the missing functions
and fails. Shared functions also need native Windows measurements: the 58 is an
inventory of Windows-only gaps, not a substitute for a Windows suite.

Android uses the ordinary Robolectric API 27/34 suite and JaCoCo 0.8.15, with its
supported Kotlin/Compose filters. As approved by the maintainer, inline bodies
count within their enclosing method; separately compiled callbacks must meet
the threshold independently. Creating a callback does not count as invoking it.
Compiler-generated scaffolding is excluded by supported native filters.

C uses GCC/gcov and the existing sanitizer-backed helper, conversion, module and
frame-retirement suites. Essential Python functions require both native line
counters and invocation evidence. Bash uses native execution locations matched
to exact source/body hashes. Private copies receive credit only when their
source is byte-identical and attribution is unambiguous. Missing measurements
fail; changed production sources invalidate the report.

## Tests and isolation

The expanded contracts exercise capture restarts and shutdown, configuration and
runtime-directory failures, owned process retirement, GUI actions, input device
construction/cleanup, decoder callbacks, malformed formats, dimensions, numeric
limits, framing, installers, EVDI setup and package assembly. Fixed, bounded
invalid-input corpora and exhaustive small ranges run in the normal suites;
these are deterministic fuzz/property cases, not a claim of exhaustive input
coverage or a long-running coverage-guided fuzzing campaign.

All tests developed during the batch are retained, including benchmark report,
collector, codec research and development-script lifecycle tests. Benchmarks and
nonessential development tools remain exempt from the 80% gate. The restored
T518 corpus-reader regression fails against the pre-fix implementation and
passes against the committed fix.

Capture uses mocks and owned stand-in processes; no EVDI display is attached.
Input constructors use a checked private syscall shim. GUI tests use a private
Xvfb session. Daemon lifecycle tests require a private PID namespace; runtime
fallback tests also require a private mount namespace masking `/run/user`.
A failed isolation prerequisite fails the test.

During development, an early runtime fixture set an invalid XDG directory and
fell back to the real user runtime directory, replacing the installed daemon's
session token. The installed service was restarted to restore consistent
credentials. The fixture was corrected to isolate `/run/user` and assert that
fallback remains inside its private directory. No token contents are retained
in the evidence. Setting a private HOME alone does not isolate runtime fallback.

## Reproduction and limits

[Coverage instructions](../../scripts/coverage/README.md) describe the pinned
reporting dependencies, source manifests and collection commands. CI is configured
to enforce the four available scopes and retain reports, manifests and native
evidence; the updated workflow has not been executed on GitHub in this batch.
The Windows workflow keeps its ACL/lease/process tests mandatory, but its native
results were not obtained during this local batch. Wine 9 rejects the protected
DACL contract and cannot stand in for that validation.

Local evidence is retained under `/tmp/uscreen-windows-work/`:

- `t497-rust-final/`, `t497-final-rust-default.lcov` and `t497-all-rust-v12.lcov`.
- `t497-android-final.json` and `t497-android-v15.xml`.
- `t497-c-gated-v12/` and `t497-essential-scripts-v13/`.
- `logs/t497-restored-*.log`, `logs/t518-permanent-red.log` and
  `logs/t518-permanent-green.log`.

The script report was exported again from the unchanged combined native database
once a retained test file was restored; counters were not altered. Production
sources remained unchanged during collection. These temporary paths are local
evidence, not published artifacts; CI regenerates its own reports.

The default and all-feature Rust suites and Clippy checks passed, as did the
Android suite, C gate, essential-script suites, restored suites, coverage-tool
contracts, formatting and the maximum-nine complexity audit. The final lint-only
rewrite of one test assertion was checked again with its targeted regression.

These checks do not measure tablet performance, battery savings, real display
presentation, native Windows security, or the unresolved Xorg crash in T222.
No APK or host binary was installed for coverage collection.
