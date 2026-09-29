T717 shared audio foundation, September 29, 2026.

Native Linux, source-matched scoped validation. No real audio device was opened.
`first-tests.log.gz`, `poll-red.log.gz` and `drift-red.log.gz` retain the failing
permanent cases before their fixes. Core/default test logs retain passing results.
The final instrumented core suite also executes all four formerly failing cases.
`audio-final.lcov.gz` and `manifest-final.json.gz` back `coverage-final.json.gz`.
The latter reports each function independently, without new scope exclusions.
Complexity is the repository's Sonar-aligned source metric, not native SonarQube.

Commands from repository root (output paths abbreviated to EVIDENCE):

```sh
cargo test --locked -p blent-config --no-default-features
cargo test --locked -p blent-config --lib audio::
python scripts/coverage/report.py snapshot EVIDENCE/manifest-final.json
CARGO_LLVM_COV_TARGET_DIR=EVIDENCE/fresh-target cargo llvm-cov --locked -p blent-config --no-default-features --lib --lcov --output-path EVIDENCE/audio-final.lcov
python scripts/coverage/report.py check --manifest EVIDENCE/manifest-final.json --lcov EVIDENCE/audio-final.lcov --scope common/src/audio --scope common/src/credentials.rs --output EVIDENCE/coverage-final.json
python scripts/complexity/check.py
cargo check --locked --workspace
python3 scripts/format-rust.py --check
git diff --check
```

Reporting Python uses the pinned `scripts/coverage/requirements.txt` environment.
Final collection used `/backups/disk2/blent-work/t717/coverage-final`, created fresh
for this source revision. Earlier target/counters were not merged into it.
No native adapters were added. T718/T719/T720 retain platform and hardware tests.
