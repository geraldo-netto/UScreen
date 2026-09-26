# Runtime directory pinning (T634)

The existing T493 rename regression exposed a Windows pinning failure. Two new
permanent regressions were added before the fix: removal must wait until the
last owner drops, and an existing delete-access handle must prevent acquiring
a pin. All three failed natively at `d264bb4` in
[run 36262176607](https://github.com/geraldo-netto/UScreen/actions/runs/36262176607).

`CreateFileW` previously requested only metadata/security access. The adapter now
also requests `FILE_LIST_DIRECTORY`, making the handle participate in ordinary
sharing checks while continuing to omit `FILE_SHARE_DELETE`. Owner-only DACL
validation and reparse-point rejection remain intact. The original rename test
and both new tests pass in the normal suite. See Microsoft's
[sharing/access contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew).

[Run 36263320613](https://github.com/geraldo-netto/UScreen/actions/runs/36263320613)
at `47efa7a` passed all 12 native Windows platform tests, full native workspace
tests, all-features MSVC build and GNU linking. Each of the private-directory
module's three production functions measures **100% executable-line coverage**;
source fingerprints exactly match the reporting checkout.

- [Failure before the fix](artifacts/2026-09-26-windows-development/t634-red.log)
- [Native passing output](artifacts/2026-09-26-windows-development/native-validation.log)
- [Per-function report](artifacts/2026-09-26-windows-development/t634-functions.json)
- [Native Windows counters](artifacts/2026-09-26-windows-development/t634-windows.lcov)

The runner uses Windows Server 2022. Windows 11 VM acceptance remains T633;
complete foundation coverage remains T493/T497. Application backends remain
unsupported until their own implementation and acceptance are complete.
