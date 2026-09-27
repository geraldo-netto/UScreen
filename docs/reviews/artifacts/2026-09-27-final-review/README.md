# Final review evidence

These artifacts accompany the [review report](../../2026-09-27-project-review.md).
The combined gate passes all 2,293 functions/methods in the accepted coverage
scope, requiring 80% executable lines for each function independently. The
single Darwin scheduling function remains explicitly excluded by the existing
T584 disposition; Linux/Windows counters do not imply macOS support.

The final manifest matches code revision `63b9627`. Coverage used native Linux
and ordinary-user Windows execution, Android Robolectric/JaCoCo, native GCC
EVDI fixtures, coverage.py function-call evidence and native Bash traces.
`capture-native.tar.gz` retains compiler-produced GCC JSON counters and compile
provenance, without executables or build directories. `scripts-native.tar.gz`
retains call observations, Bash locations and extracted-function attestations.
No changed production source is credited with an older source revision's counters.

Rust counters and exact source replacement provenance reside in the
[T651 evidence](../2026-09-27-inventory-complexity/). The full Linux default and
all-features suites and full native Windows workspace suite were collected
under T525; T650 and T651 reran all host library tests and real-command fixtures
on both platforms with fresh targets after each parser change. The combined
report uses only source-identical baseline records plus the fresh changed-file
records. Android, C and essential scripts have unchanged source fingerprints.

The full Python suite passed before T644 diagnostics were added. The entire
17-test visibility file then passed with the permanent new diagnostics tests;
see [T644 evidence](../2026-09-27-xvfb-diagnostics/). This is not a root-cause fix
for the historical runner stall.

To reproduce the final report, decompress the manifest, LCOV, JaCoCo and Python
JSON artifacts; unpack the two native archives into a temporary directory. Run
`scripts/coverage/report.py check` with:

- the final manifest;
- all eight T651 LCOV inputs (`baseline-0` through `baseline-5`, Linux, Windows);
- T651 `windows-sources.json` native source fingerprints;
- `--rust-platform linux --rust-platform windows`;
- the extracted GCC directory through `--gcov`;
- the Android XML through `--jacoco`;
- the Python JSON and extracted calls through `--python-json` / `--python-calls`;
- the extracted shell directory through `--shell`.

Use the source prefix `/backups/disk2/projects/blent` with `--prefix` when replaying
from a different checkout location. Do not add a scope filter: the retained
`combined.json.gz` covers every required language. Source bytes must match the
manifest, and unavailable counters must fail rather than receive an exemption.
