# Project rescan — 2026-09-27

Reviewed source revision: `a8c2bf395bedbcfbdd636371cc8cb66d56dfc1ea`.
Review only: no production fixes, permanent regression additions or completed
TODO removals. Findings remain open in [TODO.md](../../TODO.md).

## Scope and limits

The repository-wide inventory and searches excluded build outputs, caches,
dependencies and generated code. Focused source review covered shared Rust
configuration and wire policy; Linux and Windows host session, USB, process,
route and shutdown ownership; GUI settings and status; Android display and
camera lifecycle; C EVDI capture and frame exchange; installation, packaging,
mutation/coverage tooling, workflows and current documentation. Existing
historical review documents supplied context rather than proof of current
behavior. Generated test reports were read only as results of this validation.

This is not a claim that every source line or execution path is defect-free.
No physical tablet, native Windows session, live desktop reconfiguration,
module reload, boot, GPU acceptance or camera hardware campaign was performed.
No fresh per-function coverage measurement was collected. Existing native and
coverage requirements remain in force; passing fixtures do not establish them.

## New findings

The ledger contains reproduction conditions, required permanent regressions
and implementation constraints. Evidence strength differs by item:

| ID | Finding | Evidence in this review |
| --- | --- | --- |
| T702 | Offline Windows USB route cleanup strands session capacity/listeners | Actual portable monitor with injected ADB: attach A, make A unavailable, present B for three polls. No B session; both listeners retained. Native Windows/hardware acceptance remains separate. |
| T703 | Independent camera settings saves overwrite each other | Actual `FileConfig::merge_edits`: mirror=true followed by an independent FPS=15 edit restores mirror=false. |
| T704 | Camera status remains Streaming after frames become stale | Source trace: `frame_status` only reacts to watch changes; output and GUI preview independently expire after two seconds. No controlled-clock runtime reproduction yet. |
| T705 | Linux initial route acquisition and retirement violate ownership policy | Source trace: initial `reverse` permits rebinding; retirement unconditionally removes ports. Repair and Windows ownership paths preserve foreign mappings. No live routes changed. |
| T706 | Delayed camera-service teardown retires a newer background run | Injected `CameraBinding` sequence reproduced on Robolectric API27/API34. This demonstrates missing callback identity, not an observed physical Android service scheduling incident. |
| T707 | Camera native cleanup runs on the stop caller without a codec-use retirement barrier | Injected owned closer executes on the caller on API27/API34; source connects UI/lifecycle stop to native codec/Camera2 closure while IO drain can still be active. Native blocking and overlapping codec calls were not reproduced on hardware. |
| T708 | Existing discovery regression intermittently times out | First broad Rust run timed out in `wait_sessions`; focused rerun and later complete host-binary run passed. Cause remains unknown. |
| T709 | Reinstall overwrites configured EVDI boot capacity with two | Actual installer function with all sudo writes redirected to a temporary fixture: existing count four becomes two. No system configuration changed. |
| T710 | Current architecture description still says latency reports sort samples | Source comparison with T597 implementation: small-window sorting, ordered/reverse fast paths and order-statistic selection. Documentation-only correction. |

T702/T703 probes use the project's public production APIs. Android probes inject
ownership callbacks and an unconfined scope for deterministic sequencing; they
do not emulate native camera devices. The saved probes assert the observed
behavior. They are review evidence, not the permanent desired-behavior
regressions required before fixes.

Probe sources and results:
[Rust source](artifacts/2026-09-27-project-rescan/rust-probe.rs.txt),
[Rust observations](artifacts/2026-09-27-project-rescan/rust-probe-results.txt),
[Android source](artifacts/2026-09-27-project-rescan/android-probe.kt.txt),
[Android results](artifacts/2026-09-27-project-rescan/android-probe-results.xml),
[installer observation](artifacts/2026-09-27-project-rescan/installer-probe.log).

To repeat the Rust probe, use an external Cargo crate depending on the local
`blent` and `blent-config` crates and Tokio with `full` features. To repeat the
Android probes, add the saved Kotlin file to an external test-source directory
through a Gradle init script and run `testDebugUnitTest --tests
com.blent.ReviewCameraProbe`. Do not add these observation assertions to the
normal suite as substitutes for red/green fix regressions.

The installer probe loads `scripts/install.sh` without its final `main "$@"`,
seeds a temporary `etc/modprobe.d/blent-evdi.conf` with count four and substitutes
`sudo`: mkdir returns success, tee writes beneath the fixture root, and all
other commands fail. Calling `configure_boot_modules` produces count two.

## Validation

| Check | Result |
| --- | --- |
| `cargo test --locked --workspace --lib --bins` | Host library: 204 passed. Host binary: 382 passed, one T390 discovery timeout, three existing benchmark tests ignored. Command stopped there; not a whole-workspace pass. |
| Focused T390 discovery rerun | One passed in 0.22 seconds. |
| `cargo test --locked -p blent --bin blent` rerun | 383 passed, three existing benchmark tests ignored; 65.12 seconds. |
| `cargo test --locked -p blent-config --lib` | 129 passed. |
| `cargo test --locked -p blent-gui --bin blent-gui` | 71 passed. |
| `cargo test --locked -p blent --test evdi_modules --test evdi_helper --test conversion` | 1 + 43 + 8 passed. Injected/native helper fixtures, not live EVDI acceptance. |
| Android review probes | Four passed: two methods on API27/API34. No fresh full Android-suite pass claimed; existing T701 remains open. |
| `python -m unittest discover -s scripts/tests` in recovery venv | 307 tests reported; two failures, three import errors, one existing Windows-only skip. Missing `rpmbuild` caused both failures; missing Pillow/Xlib caused import errors. |
| Affected Python modules rerun with system Python | Visibility: 17 passed; corpus: one passed; research contracts: seven passed. RPM-dependent tests remain unvalidated in this environment. |
| `python scripts/complexity/check.py` in recovery venv | 6,423 functions measured; none above nine. This is complexity evidence, not coverage evidence. |
| Tracked Markdown relative file targets | No missing local target files found. External URLs and fragment anchors were not validated. |

The first broad Rust run overlapped other review checks. A later complete
host-binary run without that workload passed. This narrows T708 to an
intermittent failure but does not distinguish load, fixture port collisions or
production discovery behavior. Retain the assertion and investigate diagnostic
evidence before changing timeouts.

Retained logs (trailing whitespace normalized):
[initial Rust run](artifacts/2026-09-27-project-rescan/rust-tests.log),
[host rerun](artifacts/2026-09-27-project-rescan/host-bin-rerun.log),
[script run](artifacts/2026-09-27-project-rescan/script-tests.log).
Missing local RPM tooling is a validation limitation, not evidence of a package
implementation defect. No thresholds, exclusions or tests were weakened.
