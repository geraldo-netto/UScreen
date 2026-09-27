# Windows autostart native evidence (T532)

All 227 Windows workspace tests passed in the retained Windows11VM under the
ordinary, unelevated blentdev token. `result.json` identifies that context;
`tests.log.gz` retains the full native run. A fresh LLVM target collected
`windows.lcov.gz`; the guest source hashes in `sources.json` exactly match local
source bytes. The scoped per-function report passes85/85 maintained functions
in the Windows adapters plus changed shared CLI/platform files. Each of the
five autostart-policy functions measures100%; registry methods measure92.86–100%.
This is scoped native coverage, not a new whole-project combined result.

`login-red.json.gz` is the pre-implementation native failure on unsupported
`--login`. The retained permanent test passes after implementation, checking
PID/token preservation across repeated login. Native registry/GUI tests cover
state, ownership, invalid input and failure handling under temporary keys;
`registry-stress.json.gz` records30 consecutive native malformed-value passes.
No real Run preference or Windows automatic-login setting was changed by tests.

Decompress the LCOV and manifest, then at this source revision run:

```sh
python scripts/coverage/report.py check --manifest manifest.json \
  --lcov windows.lcov --native-sources sources.json --rust-platform windows \
  --scope common/src/windows/ --scope common/src/cli.rs \
  --scope common/src/platform.rs --scope host/src/windows_main.rs \
  --scope gui/src/platform/windows.rs --output replay.json
```

Use the dependencies from `scripts/coverage/requirements.txt`. Source changes
require new matching counters; do not bypass fingerprint validation. Linux
platform tests, Windows GNU cross-compilation, whole-project Rust formatting and
cyclomatic complexity checks also pass. No threshold or exclusion changed.
