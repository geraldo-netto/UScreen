# Contributing

Compatibility reports should identify the distribution, desktop/session type,
GPU/encoder, tablet, Android version, installation method and **fork commit**.
Use the [compatibility template](https://github.com/geraldo-netto/UScreen/issues/new?template=compatibility.yml).
Reports can be added to [docs/compatibility.md](docs/compatibility.md) with their
version, source and limitations; a successful build is not hardware validation.

## Bugs and questions

Use the [fork issue tracker](https://github.com/geraldo-netto/UScreen/issues).
Explain the expected result, actual result and reproducible steps. Include
`uscreen doctor` output and relevant logs: `journalctl --user -u uscreen -n 200`
for a service launch, or `~/.local/share/uscreen/daemon.log` for a direct GUI
launch. Remove tokens, device serials and personal details before posting.
For a foreground debug run, stop the existing daemon first and follow
[troubleshooting](docs/troubleshooting.md#black-screen-on-the-tablet); starting
the daemon can change the live display configuration.

## Findings, tests and commits

- Record every finding as a uniquely identified row in `TODO.md` **before**
  fixing it, preserving the existing table format.
- Record contradictions with both conflicting statements/behaviors, their
  sources and the exact resolution condition. Use **open** for actionable work.
  Reserve **blocked** for an actual missing decision, evidence or prerequisite,
  and name what is missing. Move it to open when that obstacle is removed;
  remove the row only after resolution. Continue independent work meanwhile.
- For each confirmed behavioral bug, add a permanent automated regression to
  the normal suite **before** the fix. Show it failing, then passing after the
  fix, and link it to the issue/TODO ID. Retain it after completion; do not
  delete, skip or weaken it because the bug is fixed.
- If automation is unavailable, record the exact obstacle and missing coverage
  in TODO.md and leave the bug unresolved. During a review-only task, record
  reproductions and required coverage; implement tests with the fixes unless
  tests were explicitly requested during review. Documentation/policy-only
  corrections need no artificial behavioral tests.
- Fix findings in dependency order. Make **one commit per resolved finding**
  and remove only its resolved TODO row. Preserve other open/blocked entries.
- Keep functions/methods at cyclomatic complexity **9 or less**, using
  SonarQube's cyclomatic metric rather than cognitive complexity. For shell,
  use the approved count: 1 plus branches, loops, case alternatives and
  short-circuit operators. This applies to existing project code as well as
  changes; exclude generated code, dependencies, caches and build outputs.
- Share compatible implementations and separate distinct responsibilities.
  Ask when requirements or preferences are unclear before dependent work.

## Building and validation

[Development](docs/development.md) lists the build and test prerequisites,
including native artifact tools and optional FFmpeg headers. For code changes,
run the relevant permanent regressions and normal suites:

```bash
make build
cargo test --release --workspace
cargo clippy --workspace --all-targets -- -D warnings
./android/gradlew -p android lintDebug testDebugUnitTest assembleDebug
```

For optional encoder changes, also run:

```bash
cargo test --release -p uscreen --features inproc-encoder --bin uscreen
```

Run the [cyclomatic complexity gate](https://github.com/geraldo-netto/UScreen/blob/configurable-input-devices/scripts/complexity/README.md)
for refactors. Its pinned parser setup, boundary tests and scope limitations
are documented there; CI runs it on pushes and pull requests.

Select checks appropriate to the affected components; document exact results
and obstacles. Documentation edits need link/metadata/format checks and any
automated tests that consume the edited files. Do not publish, install on the
working desktop or attach EVDI merely to validate documentation.

Measure performance claims. Host `Latency packet-ready→render-ACK` figures cover
encoded-packet readiness to render-acknowledgement receipt; they exclude
capture, encoding and packetizer assembly. Include workload, settings and
hardware; see [benchmarks](docs/benchmarks.md#how-latency-is-measured).

## Pull requests

Keep the PR scope coherent and each finding's commit separate. Explain the
problem, resulting behavior and validation. Update both ends and their tests
when the Android/host protocol changes. Distinguish isolated automated coverage
from real-device testing and list remaining limitations.

### Windows compilation preview

Run `cargo test --locked -p uscreen-config --no-default-features` first, then
`cargo test --locked --workspace` on native Windows. The Windows CI workflow
runs both, including mandatory ACL, runtime lease and owned-process tests.
Linux-only EVDI/process-group/desktop fixtures remain on Linux. GNU cross-builds
can link the workspace with `cargo test --locked --workspace --all-features
--target x86_64-pc-windows-gnu --no-run` when MinGW is available. This is a build
preview; Windows capture/input/lifecycle remain unavailable. Wine can exercise
many fixtures but does not replace native Windows security validation.
