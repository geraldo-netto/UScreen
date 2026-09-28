# T697 capacity evidence

Fresh Linux and ordinary-user native Windows counters pass each maintained
function at >=80%: six shared policy functions on Linux; eleven shared/native
functions on Windows. All 152 Linux and 153 Windows common tests pass. No skips.
Complexity: 6,723 functions, none above 9. Windows GNU test cross-check passes.

Replay decompressed counters with scripts/coverage/report.py check, manifest,
--scope common/src/capture_capacity and matching --rust-platform. Windows adds
--scope common/src/windows/capture_capacity, sources.json and prefix C:\src\blent.
SHA256SUMS hashes compressed evidence.

Native owned-buffer and joined-thread fixtures do not establish production
capture or conversion support. T528/T529 retain integration and native acceptance.
Contract: ../../2026-09-28-windows-capture-capacity.md.
