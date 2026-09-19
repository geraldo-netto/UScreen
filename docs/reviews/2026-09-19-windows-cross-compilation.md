# Windows cross-compilation check

Date: 2026-09-19. Source: `63b332e34cad62287fd2691972b8e3bab7f86685`.
Linux can produce Windows x64 binaries from the shared library today, but
the full UScreen daemon and GUI do not compile for Windows. No application
port, runtime change, deployment or Windows execution was performed.

## Scope and result

| Target | Component and operation | Result |
| --- | --- | --- |
| `x86_64-pc-windows-gnu` | `uscreen-config` library, no default features, `cargo build` | Passed |
| `x86_64-pc-windows-gnu` | `uscreen-config` library, default features, `cargo build` | Passed with MinGW tools on PATH |
| `x86_64-pc-windows-gnu` | Existing `encoder-options` example, no default features, `cargo build` | Linked a Windows PE32+ x86-64 console executable |
| `x86_64-pc-windows-gnu` | Shared-library unit-test executable, default features, `cargo test --lib --no-run` | Linked; tests **not executed** |
| `x86_64-pc-windows-gnu` | Shared library, tests and example, default features, `cargo check --all-targets` | Passed |
| `x86_64-pc-windows-msvc` | Shared library, tests and example, default features, `cargo check --all-targets` | Passed; no MSVC linking |
| Both Windows targets | Daemon and GUI, `cargo check --keep-going --bins` | Failed: 16 daemon import errors and 11 GUI import errors on each target |

The error counts describe this compiler pass, not the total porting effort:
earlier errors can prevent later diagnostics. Checking a target does not link
or run it; Cargo explicitly distinguishes this from a full build in its
[`cargo check` documentation](https://doc.rust-lang.org/cargo/commands/cargo-check.html).
No `uscreen.exe` or `uscreen-gui.exe` was produced.

The optional `inproc-encoder` feature, native EVDI C helper, packaging,
Windows driver, capture, input, USB and GPU operation were outside this check.
The tablet was not needed. Installed Linux and Android applications were not
changed, and disconnect handling was left alone after the user withdrew that
concern.

## Observed boundaries

**Shared code is already partly portable (T493).** Configuration/schema,
negotiation, version and encoder policy build for Windows. Default-feature
storage and command adapters also compile, but their semantics need Windows
work: `common/src/storage.rs:config_home` reads XDG/HOME, falling back to a
relative `.config` directory if neither is usable; `commands.rs` only creates
and signals child process groups on Linux. `common/src/lib.rs` deliberately
exports `linux` and its runtime aliases only on Linux. Compilation alone does
not establish correct Windows configuration locations, privacy, process-tree
cleanup or lifecycle behavior.

**Daemon platform dependencies are unconditional (T494).** Compiler diagnostics
point to `host/src/main.rs` and `doctor.rs` Linux imports, Unix imports in
`capture/fifo.rs`, `capture/helper.rs`, `input/linux.rs` and `osk.rs`, file
descriptors in `encoder_fifo.rs` and `stream.rs`, and missing runtime/FIFO
aliases used by capture. Source inspection also shows Linux eventfd/inotify,
uinput, EVDI, Unix signals and Linux desktop integration. The Windows checks
reach the project source with the current locked dependencies; the reported
failure is not merely a missing cross compiler.

**The GUI still calls Linux services directly (T495).** The checked eframe
dependency graph succeeds, but `gui/src/main.rs`, `settings.rs` and
`status_poll.rs` import Linux daemon, runtime, autostart, executable and pipe
services. These need a platform service interface and capability-driven
settings/diagnostics. Hiding all functionality behind empty Windows stubs
would not meet the lifecycle milestone in the integration plan.

**A compiled test executable is not Windows test coverage (T496).**
`common/src/commands.rs` unit tests spawn `sh`/`sleep` and use absence in
`/proc` as evidence of process retirement. Without a shell those fixtures
cannot start; absence of `/proc` would not establish child retirement on
Windows even with a shell installed. The XDG isolation test in `storage.rs`
unwraps HOME in its fallback cases. These are source-reviewed fixture limits,
not failures observed by running Windows tests. The existing Linux-specific
`common/tests/command_lifetime.rs` integration suite is correctly target-gated
and must remain in the Linux suite. Current CI checks portable policy on
WebAssembly but has no Windows build/test job.

All four follow-ups are recorded in `TODO.md`. Start with shared platform
services; daemon and GUI boundaries can then proceed independently. Add
regressions before each behavioral change, preserve existing Linux tests,
and extend Windows-native execution coverage as the adapters become available.
Display-driver selection and hardware validation remain separate decisions
and acceptance checks in the [Windows integration plan](../windows-port.md).

## Toolchain and evidence

This used Rust `1.98.1 (48a229cea 2026-09-01)` and Cargo
`1.98.1 (797e8a9bc 2026-08-05)` on Linux x86-64, with both Windows targets
installed using rustup. MinGW packages from the configured Ubuntu Noble
repository were downloaded and extracted under `/tmp`; no system packages
were installed. The extracted linker was GCC 13 POSIX, Binutils
`2.41.90.20240122`, and mingw-w64 `11.0.1`.

These tools were sufficient for the recorded probes. They are older than
the baseline currently documented by Rust for
[`windows-gnu`](https://doc.rust-lang.org/rustc/platform-support/windows-gnu.html)
(GCC 14.2, Binutils 2.44 and mingw-w64 12.0.0), so this is not validation of a
release toolchain. The proposed release path remains native MSVC validation;
Rust documents cross-compilation from non-Windows hosts to
[`windows-msvc`](https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html)
as potentially possible but unsupported. The MSVC checks here required neither
Microsoft linker nor SDK because they stopped before linking.

The initial default-feature GNU build failed because `dlltool` was missing.
After supplying the extracted MinGW toolchain, the unchanged library built
successfully. That setup failure is retained separately from application
source errors.

[Evidence](2026-09-19-windows-cross-compilation/) contains compressed build
logs, exact commands/results, package versions, source/lockfile provenance,
and hashes/file identification for the linked executables. No executable or
dependency cache is committed. `SHA256SUMS` covers the retained evidence files.
Local build outputs remain in
`/tmp/uscreen-windows-check-20260919/target-gnu/x86_64-pc-windows-gnu/debug/`.
The example's PE import table names Windows system libraries; no Windows
loader or execution check was performed.

## Reproduction

Install a suitable MinGW-w64 cross toolchain with its GCC, linker and
`x86_64-w64-mingw32-dlltool` available on PATH. These commands run from the
repository root and keep build products outside the checkout:

```sh
rustup target add x86_64-pc-windows-gnu x86_64-pc-windows-msvc
export CARGO_TARGET_DIR=/tmp/uscreen-windows-check
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc

cargo build --locked -p uscreen-config --lib --no-default-features \
  --target x86_64-pc-windows-gnu
cargo build --locked -p uscreen-config --lib \
  --target x86_64-pc-windows-gnu
cargo build --locked -p uscreen-config --example encoder-options \
  --no-default-features --target x86_64-pc-windows-gnu
cargo test --locked -p uscreen-config --lib --no-run \
  --target x86_64-pc-windows-gnu
cargo check --locked -p uscreen-config --all-targets \
  --target x86_64-pc-windows-gnu
cargo check --locked -p uscreen-config --all-targets \
  --target x86_64-pc-windows-msvc

# Expected to fail on the reviewed source, independently for each target:
cargo check --locked --keep-going -p uscreen -p uscreen-gui --bins \
  --target x86_64-pc-windows-gnu
cargo check --locked --keep-going -p uscreen -p uscreen-gui --bins \
  --target x86_64-pc-windows-msvc
```

Use a Windows runner for actual execution. The linked `encoder-options.exe`
only exports encoder argument policy; it does not discover Windows encoders
or demonstrate that any listed encoder can run on that system.
