# T695 cadence evidence

Fresh Linux counters pass all 37 functions in common/src/idle and
host/src/capture/idle individually at >=80%. Sixteen shared policy/control and
nine Linux adapter tests pass. Ordinary-user native Windows passes all 149
common tests, including owned Win32 event delivery/expiry/teardown. Native
counters pass all 22 common/src/idle functions. No tests skipped or weakened.
Complexity: 6,705 functions, none above 9. Windows GNU test cross-check passes.

Replay decompressed counters with scripts/coverage/report.py check, retained
manifest, the scopes above and matching --rust-platform. For Windows use
sources.json and prefix C:\src\blent. SHA256SUMS covers compressed artifacts.

The native adapter is a bounded test fixture; Windows capture integration and
physical acceptance remain T528/T529/T522. No performance or battery claim.
Contract: ../../2026-09-28-windows-idle-contract.md.
