# T651 inventory validation complexity

The T650 NUL guard raised `adb_inventory::parse` to cyclomatic complexity 10,
above the repository maximum 9. The whole-project gate reproduced the violation
before this refactor (`complexity-red.log`). Record validation now has a separate
function; parsing retains header, duplicate, accumulation and sorting policy.
No accepted inputs, rejection behavior or permanent regressions changed.

The same complexity gate passes all 6,214 functions after the refactor.
Fresh collections pass 197 Linux tests and 87 native Windows tests. Windows ran
as ordinary unelevated `blentdev`. Each of the six maintained functions in the
parser source passes the 80% executable-line requirement independently on both
platforms. The T443 and T650 malformed-inventory, bounded NUL and live-attachment
regressions remain in the normal suite.

Only `host/src/adb_inventory.rs` differs from the T650 source snapshot. All 426
other source fingerprints match. `provenance.json` identifies each baseline
input and the records removed for the changed file. Every previous parser
record is excluded before adding fresh Linux/Windows counters, collected using
new target directories. Native source fingerprints match current Rust sources.
The combined native Rust gate passes 1,426/1,426 functions.

The six filtered baselines plus `linux.lcov.gz`, `windows.lcov.gz`, the source
manifest and native fingerprints reproduce `combined-rust.json.gz` through
`scripts/coverage/report.py check`, with `common/src/`, `host/src/` and `gui/src/`
scopes and explicit Linux/Windows platforms. The baselines derive from the
[retained T650 evidence](../2026-09-27-inventory-validation/).
