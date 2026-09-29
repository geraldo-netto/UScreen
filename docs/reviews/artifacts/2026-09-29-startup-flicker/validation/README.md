# T714 validation

Base commit: `8b7a5e4`; final sources identified by `manifest.json.gz`.
Rust counters collected in a fresh LLVM target for final source bytes. The
additional existing `linux_lifecycle` integration suite ran with `--no-clean`
against the same unchanged source revision; final export includes all objects.
Android sources are unchanged between the two manifests (127 matching hashes).

- Initial regressions: three Rust failures; Android calibration UI failure on
  API 27 and 34. Same permanent T714 tests pass after implementation (mechanical
  Rust loader API updates retain their original assertions).
- Linux: 912 reported passes, including subprocess fixture reports, from
  workspace library/binary tests and selected integration suites below.
- Android: 620 tests, zero failures; lint and debug APK assembly pass.
- Native Windows: exact updater retry passes; full default workspace library/
  binary rerun has 351 reported passes, including subprocess fixture reports.
  First run had an updater fixture timeout, retained as T716. No updater test or
  production code changed. Existing two opt-in benchmark skips remain unchanged.
- Coverage: all 236 maintained Rust functions in the recorded changed-file scope
  and all 653 Android production functions reach at least 80% executable lines.
  No new exclusions. Linux native counters cover this Linux video feature and
  its shared contracts. Native Windows rerun checks shared-code compatibility;
  it does not establish Windows video support.
- Complexity: 6882 functions checked, none above 9.
- Formatting: `python3 scripts/format-rust.py --check`, `cargo fmt --all --check`
  and `git diff --check` pass.

Commands (working directory: repository root):

```sh
cargo llvm-cov --locked --workspace --lib --bins \
  --test diagnostics --test startup --test portable_session --test portable_usb \
  --test config_diagnostics --test portable_host --lcov --output-path linux.lcov
cargo llvm-cov --no-clean --locked -p blent --test linux_lifecycle \
  --lcov --output-path linux.lcov
cargo llvm-cov report --lcov --output-path linux.lcov
./android/gradlew -p android -I "$PWD/scripts/coverage/android.init.gradle" \
  profileCoverage lintDebug assembleDebug
# Native Windows, ordinary unelevated blentdev session:
cargo test --locked --workspace --lib --bins
```

Set `CARGO_LLVM_COV_TARGET_DIR` to a fresh directory before the first command;
retain it for the following two commands. Coverage reports are generated with
`scripts/coverage/report.py check`, `--rust-platform linux`, recorded scopes,
manifest and counters. Gzip files use deterministic headers. `SHA256SUMS`
protects this evidence set.

The live host AppImage and tablet APK were not replaced. Physical acceptance of
first tuning, saved-profile reconnect and native Surface/decoder presentation
remains T715. Automated checks do not claim an observed physical flicker fix.
