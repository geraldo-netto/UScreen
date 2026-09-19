# T496: Windows-native fixtures and CI

Shared command tests re-execute their own native test binary instead of calling
`sh` or `sleep`. PID readiness uses atomic publication and a bounded wait that
also observes early fixture failure. Windows verifies process exit through a
process handle; Linux keeps its `/proc` retirement assertions and original
50 ms command deadline. The Windows fixture deadline is 500 ms to allow native
executable startup; both retain the 800 ms synchronous upper bound. Linux
process-group, subreaper and delegated-work integration checks remain intact.

The first Windows run failed the shell-based async test and then hung in the
cancel test's unbounded readiness wait. The isolated test process was terminated.
The XDG regression also failed on Windows because the correct Windows path is a
known folder. That XDG test remains in the Unix suite; the mandatory T493 Windows
path test covers absent Unix environment variables and the native location.

After the fixture change, all 35 default-feature Windows library tests and
20 portable-policy tests pass under isolated Wine 9. Linux passes 59 library
unit tests, six command-lifetime integration tests and the configuration-location
test. Every workspace test executable links for Windows GNU with all features;
MSVC workspace clippy with warnings denied passes. Complexity: 4,058 functions,
none above nine. These measurements do not establish T497's coverage target.

`.github/workflows/windows.yml` adds separate native MSVC and GNU cross-build
jobs. Native Windows runs the full workspace, including the two positive private
ACL/lease checks Wine cannot validate. Those tests are neither skipped nor
weakened. The workflow has not been run remotely in this task, and native runtime
validation remains the explicit T493 blocker. No release artifact is published
by these preview jobs.

[Retained evidence](2026-09-19-windows-tests/) includes failures, local execution,
GNU linking, MSVC checking and complexity results.
