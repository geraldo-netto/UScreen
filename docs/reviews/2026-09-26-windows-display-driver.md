# T521: Windows virtual-display driver decision brief

Research checked 2026-09-26. No Windows driver was installed, selected for
production or declared supported. T527 remains the decision/validation gate.

## Candidate and distribution

Use VirtualDrivers VDD **25.5.2** as the reproducible driver-source lab candidate
(commit `3a1871fd2349c4c243a27e05a12c0babcda7dfef`). Its release offers an
x64 installer v25.05.03 and manual x64/ARM64 packages. The newer **25.7.23**
release (`d437ebc9b44a14ce6e5cc9c8b7f6beb08d6faf77`) advertises a signed
portable control app and signed display/audio drivers, but labels the control
release Beta. The driver source, INF, settings XML and license are byte-identical
between those tags in the inspected repository; control-package changes do not
prove a changed display-driver interface. Versioned [source hashes](artifacts/2026-09-26-task-batch/t521/source-hashes.json) are retained separately.
[25.5.2 release](https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/tag/25.5.2),
[25.7.23 release](https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/tag/25.7.23).

The pinned driver source is MIT-licensed: retain its copyright and permission
notice with redistributed source/substantial copies. The repository license is
not proof of every control-app, installer or third-party artifact's terms.
Prefer separate upstream installation for initial validation; no automatic
bundling or silent installation. Before any redistribution, inventory the actual
package, notices, catalog/Authenticode signatures, hashes and publisher chain on
Windows. A signed Git commit does not validate a Windows binary signature.
[Versioned license](https://github.com/VirtualDrivers/Virtual-Display-Driver/blob/3a1871fd2349c4c243a27e05a12c0babcda7dfef/LICENSE).

## Control, ownership and capture

The pinned `MttVDD/Driver.cpp` exposes the global UTF-16 named pipe
`\\.\pipe\MTTVirtualDisplayPipe`. `SETDISPLAYCOUNT` writes the shared
`vdd_settings.xml` count then invokes `ReloadDriver`; GPU selection also reloads.
`RELOAD_DRIVER` reinitializes the adapter. `FinishInit(index)` creates monitors
with connector indexes and randomly generated container GUIDs. This inspected
interface has no per-client lease or Blent-owned create/remove operation.
Changing the shared count/reloading cannot satisfy Blent's requirement to retire
only its own outputs while preserving another application's displays.
[Versioned control and monitor source](https://github.com/VirtualDrivers/Virtual-Display-Driver/blob/3a1871fd2349c4c243a27e05a12c0babcda7dfef/Virtual%20Display%20Driver%20(HDR)/MttVDD/Driver.cpp).

This is an integration limitation, not an assertion that every upstream version
lacks ownership. A local mutex or a friendly display name would not establish
cross-application ownership. An existing-driver adapter would need either a
validated upstream lease API or a maintainer-approved, exclusively provisioned
instance with stable identity; global enable/disable, count changes and uninstall
must never be ordinary Blent disconnect cleanup. Current GUI capability reporting
continues to show the Windows runtime as unsupported.

The same source consumes/releases IddCx swap-chain buffers inside the driver;
it does not expose them as a supported application capture API. Evaluate Desktop
Duplication on the actual attached output under T528. The D3D device must match
the output adapter; access denied, unsupported mode and disconnected sessions
are real API outcomes. Validate SDR geometry, rotation/cursor/stride handling,
resize, display removal, desktop lock/unlock and reconnect before adoption.
[Microsoft DuplicateOutput contract](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutput1-duplicateoutput).

## Existing driver versus owned IDD

| Option | Benefit | Responsibility and unresolved condition |
| --- | --- | --- |
| Separately installed upstream VDD | Faster lab setup; upstream supplies driver binaries, installer and updates | Exact package/signature compatibility, stable output identity, exclusive ownership and capture must be validated; current global pipe is insufficient for production lifecycle |
| Blent-owned UMDF/IddCx driver | Can define versioned create/claim/remove leases, stable tablet identities and bounded frame transport | Maintain driver, broker/API access controls, PnP/power, display modes, swap-chain/device-loss handling, installer/update/rollback, Windows test infrastructure and signing |

IddCx supplies a user-mode indirect-display model for virtual monitors and desktop
surfaces, while the driver still owns UMDF device lifecycle and PnP/power work.
It runs in Session 0; keep application-session capture/control behind explicit
Windows adapters. Do not expose Windows handles, paths or privilege policy in
shared UI/config/wire contracts.
[Microsoft IDD model](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview).

For an owned driver, agree publisher/account ownership, certificate custody,
release maintenance and supported Windows versions first. Microsoft dashboard
submissions require registered signing credentials; attestation/WHCP submission
requires a valid associated EV certificate, and SHA-2 signing. Determine the
applicable package certification/distribution path on the chosen Windows/IddCx
matrix; test signing is not a production distribution strategy. Source licensing
alone supplies none of these operational responsibilities.
[Microsoft signing requirements](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-reqs).

## Recommendation and next decision

Keep upstream VDD as a separately installed **lab candidate**. Do not integrate
its global count/reload commands into automatic attach/detach. For strict
per-application ownership, prefer an owned IDD or an upstream ownership extension
only after the maintainer accepts its ongoing signing/maintenance costs.

T527 must record that choice and prove the interface on Windows: missing/wrong
version, denied control, foreign-output collision, concurrent clients, stale
lease, crash/restart, partial-startup rollback and resize/retirement. Never remove
foreign outputs. T524 lifecycle and T522 native acceptance remain prerequisites;
T528 must validate capture compatibility separately. Source inspection and Linux
cross-compilation cannot replace these native tests. No artificial automated test
was added for this research-only item.
