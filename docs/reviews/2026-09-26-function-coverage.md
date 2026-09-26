# Per-function coverage and bounded fuzzing — 2026-09-26

This report covers maintained production functions in the accepted Linux/Windows
Rust applications, Android main app, C EVDI helper and essential installation,
EVDI setup and packaging scripts. Each function/method must independently reach
80% executable-line coverage. Aggregate percentages cannot satisfy that rule.

The archived source-matched **local** combined gate passed
**2,230 of 2,230 functions/methods**:

| Scope | Functions meeting ≥80% |
| --- | ---: |
| Rust, Linux and Windows | 1,363 |
| Android Kotlin | 593 |
| C EVDI helper, including inline headers | 153 |
| Essential Python | 61 |
| Essential shell | 60 |

The local Linux Rust measurement passed 1,283 functions. All 80 Windows-specific
production functions pass native coverage, with a minimum of 80%.
[Native run 36271915764](https://github.com/geraldo-netto/UScreen/actions/runs/36271915764)
collected revision `f5cb33cea4e198b025bb0a3a1aa5fbb0dcf42cee` on Windows Server 2022;
its application source fingerprints match the T524 implementation. Windows 11
VM acceptance is tracked separately under T633. Historical measurements do not
substitute for current source fingerprints.

The subsequent [CI run 36276441908, attempt 2](https://github.com/geraldo-netto/UScreen/actions/runs/36276441908/attempts/2)
at `9330d93` passed both Linux Rust test configurations but failed the Linux
coverage gate: `evdi_cards` measured 12/25 lines (48%) and `evdi_connectors`
17/30 (56.67%) in `host/src/vdisplay.rs`. T645 tracks deterministic sysfs coverage
on runners without host EVDI devices. Android and native Windows gates passed;
the combined CI gate was skipped. The archived local result is not evidence of
a passing remote gate. T644's earlier Xvfb startup failure did not recur in this
attempt, but its cause remains unresolved.

## Collection and permanent contracts

Both Linux workspace configurations run in separate fresh LLVM target directories.
Native Windows MSVC runs the full common/host/GUI suite, builds every feature and
collects LCOV with hashes of all Rust application source bytes. Windows-specific
functions require native execution; Linux counters or Wine do not validate them.
The combined build workflow downloads every scope and enforces one final gate.

Android runs the regular Robolectric API 27/34 suite using JaCoCo 0.8.15. Kotlin
inline bodies remain in their enclosing method; separately compiled callbacks
must satisfy their own threshold. Supported native compiler filters exclude
compiler-generated scaffolding. C uses native GCC/gcov and the normal capture,
conversion, module and retirement fixtures. Python requires invocation evidence
as well as lines; Bash requires exact source/body-hash provenance.

Permanent bounded invalid-input/property cases include:

- Rust raw descriptors, geometry/strides/slot bounds and mutation corpora in
  `common/src/raw_frame.rs`; raw transfer shape/overflow/deadline and cancellation
  contracts in `host/src/raw_transfer.rs`; malformed and oversized Windows owner/
  stop records, stale identities, partial startup and cleanup failures in the
  T524 lifecycle suites. Windows CLI numeric limits and unavailable pipe-capacity
  requests include zero, range endpoints, overflow and malformed values.
- Android input normalization and non-finite values in `InputNumericBoundsTest`,
  packet lengths/types/truncated metadata in `ChannelPacketReaderTest`, and invalid
  stream lengths in `VideoConnectionBoundaryTest`, alongside retained codec,
  camera, token and decoder lifecycle regressions.
- Capture numeric fuzz, clipped dirty-region bounds, ownership and buffer leases
  in `host/tests/evdi_helper.rs` and `host/tests/conversion.rs`.
- Existing installer, packaging, EDID and release validation suites, plus strict
  coverage-reader cases for invalid source paths/hashes, absent or partial native
  attestation, unsupported platform predicates and empty selected scopes.

These fixed corpora and bounded property cases execute in the normal suites.
They are not a claim of exhaustive input coverage or a long-running random fuzz
campaign. Existing regressions, including out-of-scope benchmark/tool tests, remain.
The archived recipe source-name bug was reproduced in a failing native Python
reporting regression before fixing the synthetic filename; its security assertions
are unchanged. Native cleanup regressions likewise failed before the T524 fix.

## Scope and evidence limits

The one Darwin scheduling function remains visible as unmeasured in the combined
report's excluded-platform list. macOS work was explicitly declined under T584;
no other platform/function exclusion was added. Unknown Rust target/feature
predicates remain in scope. Benchmarks and nonessential development tools retain
the maintainer's explicit exemption from coverage-only work.

The local Rust suites use isolated PID/mount namespaces and owned stand-ins;
Android tests do not install an APK. C fixtures do not attach a real display.
None of these counters establish physical GPU/USB/stylus acceptance, latency,
battery savings or actual Windows display/input support. Those separate TODO
items retain their exact prerequisites.

## Reporter regressions and retained evidence

Permanent tests first reproduced empty-platform vacuous success, erroneous
attestation requirements for excluded test fixtures, and omission of inline C
header functions. The same tests pass after the reporter fixes. Native source
attestation still covers every maintained Rust counter file; excluded fixtures
do not become production functions merely because LLVM reports them.

The expanded inventory includes four functions from `pixel_damage.h`, all at
100%. Their bytes match the C collector's pre-execution manifest. All original
production bytes still match the pre-execution Linux/global manifest. Adding
headers corrected the inventory, without changing production code or discarding
uncovered counters. Both manifests are retained to make that provenance explicit.

See [collector commands](../../scripts/coverage/README.md) and the
[retained evidence](artifacts/2026-09-26-function-coverage/README.md), including raw
counters, source fingerprints, the combined per-function report and regression
red/green logs. Collection reports are source-specific snapshots.
