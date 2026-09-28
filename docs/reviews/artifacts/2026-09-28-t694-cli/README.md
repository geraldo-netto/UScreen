# T694 CLI admission evidence

Permanent native `windows_cli::t694_*` tests failed before the fix: unsupported
options started a daemon and timed out; status silently ignored a port override.
The same tests pass after capability validation runs before native startup.
Shared connection policy applies supported port overrides and checks slot bounds.

Four portable CLI tests pass. Native Windows host/common suites pass 269 tests
under an ordinary user. Fresh LLVM targets and per-function reports pass all four
shared CLI functions on Linux and all 12 CLI/Windows entry functions on Windows
at >=80% executable lines each. Cyclomatic gate: 6,527 functions, none above 9.
GNU Windows cross-check and workspace formatting pass. Physical hardware was
not used; capability flags for capture and input remain unavailable.

Replay decompressed counters with `scripts/coverage/report.py check`, the retained
manifest, `--scope common/src/cli`, and the native platform. Windows additionally
uses `--scope host/src/windows_main.rs --native-sources windows-sources.json
--prefix C:\src\blent`. Source fingerprints identify measured bytes.
