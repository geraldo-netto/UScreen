# T494: Windows command-line compilation boundary

`host/src/main.rs` chooses the existing Linux supervisor in `linux_main.rs`
or the Windows diagnostics entry point. Linux EVDI, FIFO, uinput, D-Bus and
optional libavcodec dependencies stay behind target boundaries. The CLI grammar
moves to `common/src/cli.rs`; its previous Linux path remains a compatibility
export. Shared configuration, negotiation and codec policy stay in the common
crate. Windows does not run a separate copy of the streaming implementation.

The Windows executable accepts `--help` and `--version`. `doctor` reports the
known configuration location, ADB executable discovery and unavailable backends,
then exits unsuccessfully because the setup cannot stream. Start, stop, status,
Wi-Fi and display actions likewise return explicit unsupported errors, without
creating runtime state or launching a helper. GUI and native lifecycle support
remain separate milestones; passing a build does not close T493's ACL blocker.

The permanent `host/tests/windows_cli.rs` regression could not build before
this change (16 Linux import errors). Afterward all four tests execute and pass
as GNU Windows binaries under isolated Wine 9: help/version, diagnostics,
unsupported actions without filesystem side effects, and malformed/out-of-range
numeric arguments. MSVC `cargo check -p uscreen --all-targets --all-features`
also passes. The optional in-process feature is Linux-only and cannot advertise
Windows encoding support. No native MSVC executable was linked or run here.

Linux validation retains the normal suite: 367 daemon unit tests passed with
three existing ignored tests; the C/architecture integration suites passed.
The tooling run initially passed 29 tests and failed two because `rpmbuild`
was absent from PATH. The same notice and version-bump checks were repeated
with the existing extracted RPM tooling and both passed. The combined result
covers all 31 tooling tests without changing or skipping their assertions.
No tablet, installed application or active display attachment was changed.
Per-function coverage remains separately tracked in T497.

[Evidence directory](2026-09-19-windows-cli/) contains the before/after build,
Windows execution, MSVC check and Linux test logs.
