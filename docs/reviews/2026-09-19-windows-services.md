# T493: Windows shared-service foundation

The first Windows services are implemented, but T493 remains blocked on
native Windows validation of directory security and single-instance state.
Linux remains the supported runtime. No installed application was replaced.

## Implemented boundary

- `uscreen-config::platform` exposes executable discovery, runtime adapters,
  data locations and implemented-backend capabilities. Windows display, input,
  daemon lifecycle, setup, autostart and Linux tuning capabilities remain false.
- `storage::config_home` and `config_path` return errors when no safe location
  is available. Linux retains valid XDG/HOME behavior and rejects missing or
  relative bases. Windows uses the roaming known folder, without Unix
  environment-variable assumptions. A failed default store can load defaults
  but cannot write them into the working directory.
- Windows bounded commands start suspended, enter an owned kill-on-close job,
  then resume. Job assignment/resume failures retire the owned child;
  timeout/cancellation closes the job, including descendants. Linux process
  group behavior remains unchanged. Work delegated to another service is
  outside the owned tree. See Microsoft's
  [job-object contract](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).
- Windows executable discovery handles Unicode/spaces and `.exe`/`.com`
  suffixes. It does not add the current directory implicitly or accept batch
  scripts as directly executable programs.
- Windows identity includes PID, creation time and executable path. Inspection
  checks the process owner's SID; retirement validates identity using the same
  owned process handle that receives termination, preventing PID-only kills.
- The Windows runtime adapter creates an owner-only directory with a protected,
  inheritable DACL, rejects reparse points and unsafe existing permissions, and
  pins the directory against rename while owned. A stable file lock protects
  the single-instance lease; a separate bounded JSON record identifies its
  owner. Session tokens use Windows' system random source and atomic writes.
  These private-state operations require native validation below and are not
  enabled as a Windows application backend.

The default Cargo feature is named `platform`. The old `platform-linux`
selection remains an alias. `--no-default-features` still isolates portable
configuration and media policy.

## Regression evidence and limit

The permanent `common/tests/windows_platform.rs` regressions ran as real
cross-compiled Windows executables in an isolated Wine 9 prefix under Xvfb.
Before the fixes, three tests reproduced surviving descendants after sync
timeout, async timeout and async cancellation; a fourth reproduced a relative
configuration path with HOME/XDG/APPDATA variables absent. The same assertions
passed after the fixes. An additional Linux subprocess regression reproduced
the relative-path problem and passed after configuration writes became
fallible. Tests use temporary state and no tablet, driver or active desktop
attachment.

The expanded Windows integration suite reports **8 passes and 2 failures under
Wine**. The positive private-directory and runtime-lease cases fail because
Wine's queried descriptor lacks `SE_DACL_PROTECTED`, even after creation with
an explicit protected descriptor. The directory adapter correctly rejects
that state. The rejection test, owned-process checks, path tests and all three
command-retirement tests pass. Do not weaken the ACL requirement, skip these
tests in the normal Windows suite, or interpret Wine as proof of native NTFS
security semantics. A native Windows runner must run the full suite and verify
ACL inheritance, reparse rejection, token privacy and single-instance lifetime
before T493 can close.

Four additional Windows library tests pass under Wine: implemented capability
reporting, executable discovery, descriptor protection/ownership, and private-ACE truncation/mutation rejection. The in-memory descriptor test confirms that the descriptor supplied to directory creation really requests protected owner-only inheritance.
The latter checks every truncation and all alternate byte values for the
size/mask/SID fields, plus disallowed ACE kinds and inheritance flags. This is
a bounded mutation corpus, not a claim of exhaustive fuzzing. The broader
per-function coverage and fuzz requirement remains tracked in T497.

The Linux common suite passes 58 unit tests, 6 command-lifetime integration
tests and the new configuration-location integration test. Windows MSVC
`cargo check --all-targets -p uscreen-config` passes; it does not link or run
MSVC executables. Cross-compiled execution here uses GNU. Raw red/green logs
are retained in [the evidence directory](2026-09-19-windows-services/).

## Follow-on work

T494/T495 can use the validated path and discovery portions to isolate Linux
backends and build accurate unsupported-capability diagnostics. They must not
enable an unvalidated Windows daemon, display or input backend. T496 supplies
Windows-native fixtures and CI; the native private-state checks remain required.
The [Windows plan](../windows-port.md) still separates compilation from a
working lifecycle, pen input, extended display and packaging.
