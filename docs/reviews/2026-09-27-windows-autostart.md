# Per-user Windows autostart (T532)

Native acceptance passed in the retained Windows 11 VM under ordinary,
unelevated `blentdev`: 227 workspace tests pass. The scoped native coverage gate
passes 85/85 functions, with all five autostart-policy functions at 100%.
[Source-matched evidence and replay](artifacts/2026-09-27-windows-autostart/README.md)
retain the failing pre-implementation login test, passing native suite and counters.

The GUI's **Start at login** preference uses a single per-user Windows Run value,
`io.github.geraldo_netto.blent`. Its command quotes the absolute `blent.exe` path
and invokes the internal `--login` launcher. Paths retain their UTF-16 characters,
including spaces and non-ASCII names. Commands beyond Windows Run's documented
260-character limit are rejected before registration changes. No Windows service,
driver installation, or elevation is involved in ordinary login startup.

The launcher reuses the existing native daemon lifecycle. An already-ready
instance retains its PID and token; a missing daemon is launched independently
and must reach readiness. Combining the login launcher with another command is
rejected. Display, pen, camera and tray support remain separate capabilities.

Registration reads verify the expected command shape and existing executable.
A missing or stale executable does not report enabled. Disable can retire a
recognized stale registration without finding an executable; upgrade replaces
the same owned value. Unrelated value names remain untouched, and foreign data
at Blent's name is rejected rather than overwritten. The native adapter rejects
wrong registry types, oversized strings, embedded NULs and malformed registry strings. `Registration::set_enabled` is the reusable update/removal entry point
for future T534 packaging; installer format and distribution remain undecided.

Permanent tests cover enable/disable and idempotence, Unicode/spaced and stale
paths, upgrade replacement, foreign entries, permission failures, malformed
registry values, bounded invalid-command mutations, GUI preference readback and
repeated login. Tests use private registry keys and temporary runtimes. A native
pre-implementation login test failed on the unsupported flag; the implemented
launcher passes the same identity/token assertions. Registry fixtures retain
initialized padding so an unterminated input cannot accidentally acquire a NUL
from adjacent allocator memory through `RegSetValueExW`.

Native test commands:

```powershell
cargo test --locked --workspace t532
cargo llvm-cov --locked --workspace --remap-path-prefix --lcov --output-path windows.lcov
```

Reference: [Microsoft Run and RunOnce registration](https://learn.microsoft.com/en-us/windows/win32/setupapi/run-and-runonce-registry-keys).
