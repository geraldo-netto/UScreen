# Mutation testing (T652)

T652's accepted scope is **complete tooling and bounded campaigns** across maintained
Rust, Kotlin, C and essential Python/Shell. Exhaustive execution of every generated
mutation was explicitly not selected. Neither engine reports a whole-project score;
there is no agreed mutation-score threshold. Keep the independent
[80% per-function native line gate](../coverage/README.md), existing platform limits,
and benchmark/development-tool exemptions unchanged.

The [completion evidence](../../docs/reviews/2026-09-27-bounded-mutations.md) records
exact samples, baselines, outcomes, semantic review and unselected scope. Historical
Rust campaigns retain equivalent survivors and unviable edits as distinct outcomes.
A strict campaign passes only when its nonempty, complete inventory is all caught
and the unmodified baseline passes. A timeout, build failure, unrelated test error
or missing evidence is never a caught fault.

## Reviewed bounded faults in every maintained language

`catalog.json` specifies exact production-source edits, operator descriptions,
native platforms, separate build/test commands and the expected failing normal
regression. Both baseline and mutations use the same commands. `bounded.py` copies
tracked and nonignored working files into a fresh external directory, fingerprints
all source bytes, rejects ambiguous/no-op/escaping edits, retains each diff/log,
and restores the private source after every candidate. A changed source snapshot
fails the report; a later change in the live checkout is reported separately.

```sh
python3 -m unittest discover -s scripts/tests -p 'test_*mutation*.py'
python3 scripts/mutation/bounded.py --profile python --output /absolute/new/evidence
```

| Profile | Native execution | Selected operators and normal suite |
| --- | --- | --- |
| `python` | Linux and ordinary Windows | EDID checksum arithmetic and invalid physical-size boundary; `test_gen_edid.py` |
| `shell` | Linux | EVDI capacity arithmetic and invalid argument admission; `test_evdi_setup.py`, with fake system commands |
| `capture-c` | Linux | Dirty-mask index and chroma coefficient; normal sanitized conversion/oracle integration tests |
| `kotlin` | Linux JVM/Robolectric | Version ordering and numeric overflow boundary; the permanent shared-version fixture regression |
| `linux-include` | Linux | IP predicate and boolean operator directly in `linux_main.rs`; normal `t511` invalid/bounded corpus |
| `windows-tray` | Ordinary interactive Windows user | Quit delivery and stale-action guard; normal native T531 tray/shared-state regressions |

Linux requires Bubblewrap and working unprivileged user/PID/network namespaces.
Every build/test runs with the host filesystem read-only, only its campaign output
writable, a private `/tmp`, no host network and private HOME/config/cache paths.
Cargo/Gradle dependency caches are copied before execution; no hardlinks expose
original writable cache inodes. Warm required dependencies first (`cargo fetch
--locked`, or the ordinary Gradle build/unit test). Kotlin additionally needs JDK
17 and `ANDROID_HOME` pointing to the installed SDK; ignored `local.properties`
and signing material are never copied. Its JVM tests do not validate a tablet.

Windows uses reviewed normal-suite adapters: isolated fixture executables,
watch channels and owned windows/icons/menus. Run `windows-tray` in an ordinary
interactive Explorer session with Cargo, Git and Python 3.12 available. The native
collection also runs a separate-process-group descendant deadline regression; its
negative control removes `/T`, fails, and is restored before the passing suite.
It never changes Explorer or another application's tray resources. The Linux
namespace and Windows ordinary-session mechanisms are explicit platform boundaries.

Defaults are four compiler jobs, 600 seconds per build, 180 seconds per test phase
and a 3,600-second execution deadline. Adjust `--build-jobs`, `--build-timeout`,
`--test-timeout` and `--campaign-timeout` for available hardware. Bounded candidates
run sequentially; each campaign needs private source, dependency-cache and build
space. Existing output directories are rejected. Process-tree cleanup is bounded;
failed/missing launches, deadlines and incomplete tests remain visible. The
manually dispatched `mutation-bounded.yml` runs the Linux profiles and retains
artifacts. Native interactive Windows validation uses the same CLI separately.

Rust failure identity comes from both test status lines and final failure summaries;
compiler stderr can interrupt a status line. Python requires an actual unittest
failure, not merely a process exit. Kotlin uses freshly cleared and then retained
JUnit XML; zero/skipped-only results do not pass. C must compile successfully with
the normal sanitizer/warning flags before its Rust-driven native test can count
as caught. Review the actual assertion for every caught result; a matching test
name alone is not proof that the selected fault caused its failure.

## Generated Rust campaigns

The existing `run.py` profiles retain cargo-mutants 27.1.0 discovery/execution:

```sh
cargo install cargo-mutants --version 27.1.0 --locked
python3 scripts/mutation/run.py --profile usb --output /absolute/new/rust-evidence
```

`usb` supports Linux and Windows. `linux-autostart` and `windows-autostart` reject
foreign platforms. `shared-policy`, `shared-video` and `android-commands` select
portable policy and explicit host consumers. Rust Android commands do not measure
Kotlin. These latter three generated campaigns have Linux evidence; their full
Windows campaigns are unmeasured and are outside the accepted bounded requirement.

Defaults: two workers, six compiler jobs, 60-second test timeout, 600-second build
timeout and a two-hour campaign deadline. Adjust `--jobs`, `--build-jobs`,
`--test-timeout`, `--build-timeout`, `--campaign-timeout`. More workers use more
memory/disk. The runner records hashes and commands before cargo-mutants works in
further private copies; no mutation is applied to the live checkout. The manual
`mutation.yml` workflow preserves this evidence separately from line coverage.

T671: cargo-mutants 27.1.0's `--test-package` alone does not apply to the baseline.
The runner supplies explicit Cargo package arguments too, and the importer rejects
missing/different baseline and mutant commands. See the
[permanent regression](../../docs/reviews/2026-09-27-mutation-baseline-scope.md).

## Complete scope listing and discovery limits

```sh
cargo mutants --no-config --list --json --workspace > /outside/cargo-inventory.json
# Use the environment containing scripts/coverage/requirements.txt.
python scripts/mutation/scope.py --cargo-inventory /outside/cargo-inventory.json \
  --output /outside/scope.json
python -m unittest discover -s scripts/mutation -p test_scope.py
```

The maintained coverage inventory, not engine discovery, determines this listing.
It retains every maintained file's hash/function count, selected bounded edits,
files without generated Rust candidates, native Rust file reachability and
`include!`-only paths. File reachability does not prove execution of every mixed
`cfg`/feature branch. Unselected files remain explicitly unselected. Macro bodies
and unsupported operators are not exhaustive fault coverage.

The Rust engine does not expand `include!`. The bounded runner resolves execution
for this boundary by editing the actual `linux_main.rs` span and compiling the
original crate graph, including its original guards and tests. No rewritten crate
root or synthetic Windows compilation is credited. Other include descendants are
listed, not silently counted as tested. Foreign-only Rust files are classified by
native reachability and must use the matching native profile.

For survivors, verify that existing integration consumers were selected before
adding tests. Explain equivalence concretely or record actionable missing assertions
in TODO.md. Keep regressions in the normal suite; behavioral fixes require a failing
regression before changing production code. Never weaken tests or hide unviable edits.
T664 found an unrelated fixture failure mislabeled as caught; T666 reran a transient
Windows linker failure. Retain raw outcomes and correction evidence, including the
bounded runner's interleaved-output parser regression in the completion report.

Engine references: [configuration](https://mutants.rs/config-file.html),
[exit codes](https://mutants.rs/exit-codes.html). No physical USB/display/camera
acceptance or declined hardware/performance campaign is implied by these results.
