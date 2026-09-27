# Host-independent sysfs coverage (T645)

The normal Cargo integration suite now mounts a temporary sysfs fixture inside
its own user/mount namespace. It never modifies host sysfs or attaches devices.
Empty and missing roots, non-EVDI devices, duplicate cards, invalid UTF-8,
malformed/overflowing indices, u32 boundaries, connection states and missing
EDID/status files all exercise the production discovery functions.

The unchanged Linux 80% per-function gate first failed using the two complete
CI collections from run36276441908 attempt2 (revision9330d93): 1281/1283 passed;
evdi_cards12/25 and evdi_connectors17/30. All baseline source hashes still match;
the only inventory addition is the new integration test, as recorded in
source-comparison.json. A fresh LLVM target ran `cargo llvm-cov --locked -p blent
--test vdisplay --lcov --output-path fixture.lcov`. Adding those native fixture
counters makes the same gate pass1283/1283, with both discovery functions100%.

LCOV source headers are normalized to repository-relative paths for replay;
no counters, uncovered lines, thresholds or exclusions were changed. This is a
local source-matched combined result, not a claim of a new GitHub run. The user
requested local commits without pushing. Full-suite baseline counters are
retained in default.lcov.gz and all-features.lcov.gz; fresh fixture counters,
manifest, red/green reports and test output are retained alongside them.

At this source revision, decompress the files into a temporary directory and
run the unchanged `scripts/coverage/rust_check.py` with `--manifest manifest.json`,
all three `--lcov` files and `--output <directory>`. Omitting fixture.lcov
reproduces the two failing functions. Use the coverage requirements venv from
`scripts/coverage/README.md`. Later source changes require a fresh snapshot and
matching counters; do not bypass fingerprint verification.
