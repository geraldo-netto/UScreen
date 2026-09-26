# Per-function coverage (T497)

T497 remains blocked on native Windows evidence. Linux Rust, Android, capture C
and essential scripts meet the per-function threshold; the full cross-platform
requirement does **not** yet pass. See the [measured scope and limits](../../docs/reviews/2026-09-19-function-coverage.md).
The requirement covers Rust applications, the Android app, the C EVDI helper,
and essential installation, EVDI setup and packaging scripts. Benchmarks in any
language (including their Rust example adapters) and nonessential development
tools are exempt. The script inventory is
explicit in `inventory.py:essential_script`: packaging sources plus the release
builder, bundle staging, distribution-document copying, EDID generation, APK
verification, installer, EVDI setup, desktop/systemd launch writers and the CI
artifact/Arch builders and package ABI validation. All tests already developed,
including benchmark and development-tool tests outside this scope, remain in
the normal automated suites.
The capture C gate currently covers every maintained function in `host/evdi/`;
this is a scoped result, not a whole-project percentage.

Install the reporting tools in an isolated environment:

```sh
python3 -m venv /tmp/blent-coverage-venv
/tmp/blent-coverage-venv/bin/python -m pip install -r scripts/coverage/requirements.txt
/tmp/blent-coverage-venv/bin/python -m unittest discover -s scripts/coverage -p 'test_*.py'
```

## Capture C gate

```sh
/tmp/blent-coverage-venv/bin/python scripts/coverage/capture_check.py /tmp/blent-capture-coverage
```

Use a new output directory for each run. The command runs the normal helper,
conversion, EVDI-module and frame-retirement test targets with GCC counters,
preserves coverage notes before the fixtures remove their temporary executables,
and rejects changed sources. It writes the test log, raw evidence, source manifest
and `report.json`. Every capture function must have at least 80% executable-line
coverage. Missing counters fail the gate. These tests simulate EVDI; they do not
attach a virtual display or validate the unresolved Xorg crash in T222.

## Python and shell collection

```sh
/tmp/blent-coverage-venv/bin/python scripts/coverage/script_check.py /tmp/blent-script-coverage
```

This runs the normal isolated tooling regressions and Python test suites,
including retained tests outside the coverage scope. Only the essential-script
inventory is subject to the threshold.
Packaging prerequisites, including `rpmbuild`, must be available. `--report-only`
allows a report containing gaps to be written without failing the command; the
report still says `passes: false`. Remove that option to enforce the threshold.
Test failures always fail collection.

Python uses coverage.py subprocess counters and separate function-call evidence.
Loading a definition, including a one-line function, is not counted as invoking
it. Byte-identical copied fixtures are attributed to their original sources by
SHA-256; changed or ambiguous copies do not receive credit. Bash uses native DEBUG
locations matched to the source hash. Extracted Bash functions also count when
their entire definition is byte-identical to exactly one maintained function;
the collector records a body hash and original line bounds before the temporary
fixture disappears. Changed or ambiguous bodies receive no credit, and failed
attestation fails the report. The trace records only source hashes,
filenames and line numbers, never expanded commands, arguments or credentials.
Shell coverage applies to Bash executions; code executed only by another shell
remains unmeasured. Embedded Python and RPM scriptlet functions remain in the
inventory even when the collector cannot yet measure them.

## Linux Rust gate

Install `cargo-llvm-cov` 0.8.7 and the Rust `llvm-tools-preview` component.
With the snapshot described below and a new evidence directory, run both normal
workspace configurations against unchanged production sources:

```sh
cargo llvm-cov --locked --workspace --lcov --output-path /tmp/blent-default.lcov
cargo llvm-cov --locked --workspace --all-features --lcov --output-path /tmp/blent-all-features.lcov
/tmp/blent-coverage-venv/bin/python scripts/coverage/rust_check.py \
  --manifest /tmp/blent-coverage-manifest.json \
  --lcov /tmp/blent-default.lcov --lcov /tmp/blent-all-features.lcov \
  --output /tmp/blent-rust-coverage
```

The scoped Linux gate writes `linux.json`. `all-platforms.json` retains every
foreign-platform function and fails until native coverage is supplied to the
combined reporter. Classification evaluates explicit `cfg` target predicates,
including nested `all`/`any`/`not`, regardless of argument order. Only code
provably unavailable on Linux leaves the Linux gate; unknown feature predicates
and shared module references stay in scope. Guarded module descendants inherit
the restriction. Missing Linux functions always fail, and foreign-platform
counters are rejected from a Linux collection. This gate does not establish
Windows or macOS runtime support or coverage (T493/T497/T583). CI uploads the
manifest, raw counters, reports and logs as coverage artifacts.

## Combined native reports

Before measuring, create a source snapshot:

```sh
/tmp/blent-coverage-venv/bin/python scripts/coverage/report.py snapshot /tmp/blent-coverage-manifest.json
```

Keep sources unchanged until the measurements and report are complete. Collect
Rust with `cargo llvm-cov --workspace --all-features --lcov --output-path ...` and
also without `--all-features`; preserve both reports to cover the CLI-only build.
GCC records use `compiler.py`, as demonstrated by the capture gate. Android uses
the regular Robolectric suite with a separate JaCoCo init script:

```sh
./android/gradlew -p android -I "$PWD/scripts/coverage/android.init.gradle" profileCoverage
```

The Android XML is written to
`android/app/build/reports/jacoco/profileCoverage/profileCoverage.xml`.
Enforce the method gate with:

```sh
/tmp/blent-coverage-venv/bin/python scripts/coverage/report.py check \
  --manifest /tmp/blent-coverage-manifest.json \
  --jacoco android/app/build/reports/jacoco/profileCoverage/profileCoverage.xml \
  --scope android/app/src/main/ --output /tmp/blent-android-coverage.json
```

This does not install an APK or use a physical tablet. Its scope is the main
Android app; separate benchmark applications are exempt.

Pass the native artifacts to `report.py check`; see `--help` for repeated LCOV,
GCC, Python and shell inputs. An explicit `--scope` is recorded in the output.
Without one, every maintained production function in the required scope is checked. The source
inventory excludes generated files, dependencies, caches/build outputs and test
fixtures, including Rust test-only modules. An absent measurement fails; there
is no aggregate-percentage substitute or silent platform exclusion.

Kotlin uses authored PSI functions, accessors, constructors and callbacks compiled
as separate methods. Per the maintainer's scope, compiler-inlined lambda bodies
count within their enclosing function/method. Lambda body-entry lines disambiguate
callbacks declared on a parent's first statement line. Native method counters
prevent allocating a callback from counting as executing it. Multiple compiled
variants must all pass. JaCoCo 0.8.15 applies its supported Kotlin inline and
Compose filters, excluding generated scaffolding; see its
[change history](https://www.jacoco.org/jacoco/trunk/doc/changes.html).

The Linux daemon lifecycle regression requires a private PID namespace. It uses
`unshare` by default. The portability CI container supplies its own namespace and
sets `BLENT_TEST_PRIVATE_PID_NAMESPACE=1`; do not set that variable in a normal
desktop session. The regression starts only its own isolated daemon, uses fake
ADB/helper commands and signals its owned child directly.

Runtime-directory failure tests also require a private mount namespace and mask
`/run/user` with a temporary filesystem. Invalid XDG paths can fall back to the
real user runtime directory; setting a private HOME alone does not isolate that
case. These tests may use the existing isolated CI container when its explicit
private-namespace marker is present. A namespace setup failure fails the test.

Native Windows ACL/instance-ownership evidence is still blocked under T493.
Wine loses the protected-DACL flag and cannot validate the required positive
cases. Keep these tests and unmeasured functions visible; do not treat a Linux
report or Windows cross-compilation as native Windows coverage.
