# Autostart removal error regression (T656)

The normal Linux autostart/AppImage suite missed a mutation that accepted every
filesystem removal failure as if the desktop entry were already absent.
`t656_disable_reports_nonmissing_filesystem_errors` now creates a directory at
the desktop-entry path, requires disabling to report an error, and verifies its
unrelated contents remain intact. Removing that fixture then verifies missing-file
disable remains idempotent. The subprocess uses private HOME, XDG configuration
and a fake systemctl; it never changes real login settings.

The new permanent test fails when the `NotFound` match guard becomes `true`,
passes against unchanged production code, and all six normal autostart tests pass.
[Evidence](artifacts/2026-09-27-mutation-resume/t656/) retains baseline and mutant
logs, source hashes, diff and outcomes. This fixes a test gap, not production behavior.
