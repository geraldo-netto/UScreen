# T505 — offline SDK fixtures for bundle regressions

T250's required APK verifier exposed stale distribution fixtures: their mock
Gradle writes placeholder APK bytes, but they supplied no matching SDK reports.
The existing T102 success case failed with “Android SDK missing” before reaching
its archive/helper assertions. Production release verification remains required.

Publication, bundle and notice fixtures now share deterministic signer/manifest
tools inside each temporary test directory. The existing wrong-certificate
publication test still replaces that signer and verifies rejection before API
writes. No real APK is accepted through these test-only tools.

The same distribution suite now passes (two tests), notices pass (one test),
and all 16 offline publication/metadata tests pass. The regressions remain in
the normal Cargo tooling suite. [Red/green logs](artifacts/2026-09-19-packaging-fixtures/)
retain the evidence; RPM checks used the existing isolated local RPM toolchain.
