# T696 release notification evidence

Fresh Linux counters pass all 31 scoped production functions at >=80% individually.
The final host/common library suites pass 271/152 tests. Windows full host/common
unit and integration suites pass (159/153 library tests plus native integrations).
Two existing opt-in library benchmark ignores remain unchanged. Windows GNU test
cross-check and the final Linux binary compile pass. The Linux binary suite also
passed 384 tests before the equivalent polling-wrapper consolidation; its three
pre-existing benchmark ignores remain unchanged.

The first native attempt exposed virtual-clock fixture startup races. Fixtures
now let spawned tasks register their timer before advancing the paused clock;
production delays and every cleanup assertion remain unchanged. Three repeated
native polling runs pass. Fresh full native collection then passed. Coverage
identified the remaining tray callback error path; a permanent native test now
proves failed Settings/release launches preserve the tray and Quit. A second
fresh target collected that four-test tray suite. Production sources are byte-
identical between both native collections; only the test module was extended.
Combined native counters pass all 81 scoped functions. No production source,
exclusion or retained assertion was weakened to meet coverage.

Failing-first logs retain oversized valid release JSON and invalid SemVer erasing
an existing notification. Final green suites cover both regressions, no-network
fixtures, optional polling, active-child cancellation, fixed URL arguments,
native menu/icon ownership and retained real FFmpeg probe calls. No browser was
launched and no release API call was needed by these tests.

Complexity: 6,752 functions, none above 9. Replay decompressed evidence with
scripts/coverage/report.py check, manifest.json and matching --rust-platform.
Shared scopes: host/src/update, host/src/command_output.rs,
host/src/encoder_probe, host/src/tray_state.rs, common/src/version.rs.
Windows adds host/src/windows_tray, host/src/windows_main.rs and
common/src/windows/paths.rs; supply both native LCOV files, sources.json and
prefix C:\src\blent. full-sources.json preserves the first native attestation.
SHA256SUMS hashes compressed artifacts. Contract:
../../2026-09-28-windows-tray-updates.md.
