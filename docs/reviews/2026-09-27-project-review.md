# Project implementation and review — 2026-09-27

Implemented T645, T532 and T525 in separate commits, then repeated repository
review and regression validation. The final pass found no additional actionable
findings. This is the result of the checks and boundary review below, not a claim
that testing proves the absence of all defects. All commits remain local.

## Implemented and corrected

| Item | Commit | Result |
| --- | --- | --- |
| T645 | `a39f0d7` | Isolated native sysfs fixtures cover EVDI discovery without changing host devices. Both discovery functions reach 100%; the original two-function coverage failure is retained. |
| T532 | `1e6ffa9` | Optional per-user Windows autostart uses an owned HKCU Run value and idempotent `--login` lifecycle launch. Native tests cover Unicode/space paths, stale or foreign values, upgrades, registry bounds and GUI integration. |
| T525 | `252b32e` | Shared USB discovery, protected credential delivery, reverse-route ownership, reconnect/token retirement and bounded cleanup integrate with the Windows daemon. CLI/GUI read producer-owned assignment state; unsupported display/input capabilities remain explicit. |
| T648 | `6ffe4c8` | Architecture and scheduling documentation reflects the implemented Windows lifecycle and native scheduling validation; macOS retains its declined disposition. |
| T649 | `e4f69ee` | Five historical Android source links follow the `com/blent` package location. |
| T650 | `b840b7b` | Embedded NUL in ADB inventory is rejected as unknown, preserving an existing attachment and credential. Permanent bounded parser and live authenticated-socket regressions first failed, then passed. |
| T644 | `61c7d49` | Permanent Xvfb timeout diagnostics retain owned-child state before cleanup; regressions prevent inspecting a reaped PID. The historical stall remains unresolved and blocked on a recurrence with evidence or equivalent deterministic reproduction. |
| T651 | `63b9627` | Separate ADB record validation keeps parser complexity within 9 without changing its contract; existing malformed-input regressions remain intact. |

Resolved rows were removed individually. T644 remains in Other items with its
exact missing evidence. T531 moved to Open because T525 supplies its shared
state prerequisite; T529 now names only the remaining capture/driver dependency.
Unrequested future features and existing blocked, deferred or declined work
were not treated as completed.

## Review scope

The repository-wide scan excluded generated files, dependencies, caches and build
outputs. It covered maintained Rust applications/shared contracts, Android app,
C EVDI helper and optional GPU prototype, Python/Shell tooling, packaging, CI and
documentation. Follow-up review concentrated on command argument handling,
attachment and authentication retirement, route cleanup, runtime ownership,
configuration/capability consistency, Android control/session boundaries and
native adapter isolation. The normal suites retain bounds, malformed-input,
ownership, cancellation and lifecycle regressions.

Whole-project complexity scanning checked 6,214 functions with no score above 9.
Rust formatting and diff-whitespace checks passed. Local Markdown targets and
TODO section/status/unique-ID checks passed; the final link scan is retained
with the evidence. No merge-conflict markers or unfinished production macros
were found in the application/tooling scan; the matching `todo!()` text belongs
to an intentional parser-test fixture.

## Validation

- Linux: 887 default and 757 all-features workspace tests passed. A real native
  ADB command fixture passed; after each parser change, fresh targeted collection
  reran 197 tests including the full host library and native command fixture.
- Windows 11: 244 workspace tests passed as ordinary unelevated `blentdev`, plus
  the supplemental native command/daemon fixtures. Fresh T650 and T651 collections
  each passed 87 host-library and native-fixture tests. Native source fingerprints
  match the corresponding source revision; Wine and cross-compilation supply no
  native coverage credit.
- Android: `lintDebug profileCoverage assembleDebug` passed. No APK was installed
  and no physical tablet settings were changed.
- Native C EVDI collection passed its helper, conversion, module and frame-retirement
  suites. Tests simulate EVDI without attaching a host display. GPU prototype
  sanitizer/bounds tests ran in the normal Linux suite; the accepted C coverage
  scope remains `host/evdi/`.
- Script collection passed 36 tooling tests, 300 Python tests, 9 complexity tests
  and 67 coverage-tool tests. After T644 diagnostics, all 17 visibility tests passed.

The final combined gate requires **at least 80% executable-line coverage for every
function/method**, independently:

| Required scope | Passing / measured |
| --- | ---: |
| Rust, native Linux + Windows | 1,426 / 1,426 |
| Android | 593 / 593 |
| C EVDI helper | 153 / 153 |
| Essential Python scripts | 61 / 61 |
| Essential Shell scripts | 60 / 60 |
| Total | 2,293 / 2,293 |

All coverage collections retain source fingerprints and raw native evidence.
Changed source files replace all earlier counters for those files; only exact
unchanged sources reuse previous full-suite records. Each Rust recollection used
a fresh target directory. See the [combined evidence and replay instructions](artifacts/2026-09-27-final-review/README.md),
[USB report](2026-09-27-windows-usb.md), and
[autostart report](2026-09-27-windows-autostart.md).

Windows USB validation used a compiled native ADB fixture in the retained VM.
Physical tablet/GPU/display/input acceptance remains unavailable under T522;
native capture/input are still unsupported. Actual Explorer logoff/reboot
activation was not exercised. The VM password and automatic-login preference
remain as requested. No commits were pushed.
