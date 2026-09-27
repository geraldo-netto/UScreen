# Windows 11 development VM (T633)

The retained VM passed final acceptance on 2026-09-27 Europe/Rome. It lives
outside Git at `~/.local/share/blent-dev/windows-vm/`. Open **Blent Windows 11 Dev**
from the Linux application menu, or run its `open.sh`. Preserve the disk,
firmware, TPM state, credentials and setup files. The one-time installation
marker must not be removed or bypassed.

## Accepted configuration

- Official Windows 11 Enterprise 25H2 x64 evaluation, build 26200, 90 days.
- KVM, 8 virtual CPUs, 16 GiB RAM, sparse 128 GiB disk, UEFI and TPM 2.0.
- NAT networking; no exposed ports, host-folder sharing, or physical USB/GPU
  passthrough. The Linux desktop and connected tablet remain independent.
- MSVC Build Tools 2022 17.14.37710.0 and Windows SDK 10.0.26100.0; Rust/Cargo
  1.98.1, MinGit 2.55.0.windows.5, and stock FFmpeg 8.1.2 CLI.
- VirtIO serial driver and QEMU guest agent; no Visual Studio IDE or full guest
  tools bundle.
- Removable consumer/gaming/Copilot apps removed; Search/indexing, web/cloud
  search suggestions, widgets, consumer promotions and Game DVR disabled.
  Protected shell packages remain installed for servicing.
- At the maintainer's explicit request, `blentdev` retains automatic login and
  promptless administrator elevation. UAC remains enabled. Password and account
  state are kept in the private VM setup, outside Git.

The Microsoft ISO is 7,092,807,680 bytes and passed SHA-256 verification:
`a61adeab895ef5a4db436e0a7011c92a2ff17bb0357f58b13bbc4062e535e7b9`.
Source: [Microsoft evaluation](https://www.microsoft.com/en-us/evalcenter/evaluate-windows-11-enterprise),
[official ISO](https://aka.ms/Win11E-ISO-25H2-en-us),
[published hashes](https://aka.ms/Win11-Hash-PDF).

## Native validation

On application revision `9330d93a295cbe348c253f82caf105a16333775f`, all three
commands returned zero:

```powershell
cargo test --locked -p blent-config --no-default-features
cargo test --locked --workspace
cargo build --locked --workspace --all-features
```

The existing seven Windows lifecycle regressions also passed under an ordinary,
unelevated `blentdev` token, covering owned start/stop/restart, duplicates,
crash recovery and cleanup. The full initial toolchain checks ran elevated.

Final native checks confirm TPM present/ready, Secure Boot, indexing stopped and
disabled, Search policy and the actual user's taskbar setting, enabled automatic
login, and the requested UAC policy. Win+S did not open Search during a guest-only
UI check. A fresh VM stop/start returned to the desktop automatically. The guest
clock differs from the host by one second; the launcher uses `-rtc base=localtime`
as documented by [QEMU](https://qemu-project.gitlab.io/qemu/system/invocation.html).

Only the nonremovable `Microsoft.XboxGameCallableUI` remains from the targeted
consumer app set. Effective AppLocker policy denies that package and permits the
shell. Ordinary-user activation returned `0x800704EC` with process ID zero, and
AppLocker event 8022 recorded that Xbox was prevented from running. This is actual
activation evidence in addition to policy evaluation. See Microsoft's
[activation API](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-iapplicationactivationmanager-activateapplication)
and [event definitions](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/applocker/using-event-viewer-with-applocker).

## Retained setup regressions

Each confirmed setup defect has a retained automated regression, run failing
before the fix and passing afterward:

- Registry key creation erased sibling policy values. `Set-Dword` now creates
  only missing keys.
- Synthetic wildcard matching claimed an Xbox denial that Windows did not
  enforce. Native AppLocker evaluation now tests exact package names, actual
  publishers, unrelated allowed packages and version boundaries.
- Native policy merging kept obsolete rules with the same IDs. The update
  replaces owned IDs while preserving unrelated rules and collections; repeated
  updates are idempotent.
- A disconnected status reader could leave a QGA-owned file handle open and
  prevent bootstrap status writes. Monitoring now uses bounded process-owned
  reads with write/delete sharing. Lost-response and invalid-result tests run
  in the Python suite; native disconnect/write checks reproduce the old lock
  and verify the replacement.
- Windows PowerShell treated Cargo's normal stderr as a terminating error.
  `Invoke-NativeCheck` logs stderr and uses the native exit code.
- The default UTC virtual RTC left Windows two hours behind after a restart.
  The permanent native clock assertion failed at -7,200 seconds and passed
  after configuring the Windows-compatible RTC and restarting the existing VM.

[Tooling and commands](../../scripts/dev/windows-vm/README.md) describe the
normal Python/Windows CI regressions and the separate native VM acceptance
suite. These are nonessential development tools under the existing coverage
exemption. No production coverage threshold or exclusion changed.
[Sanitized evidence](artifacts/2026-09-27-windows-development-vm/README.md) retains
native results, tool versions, regression outcomes and source hashes.
Earlier guest log timestamps precede the clock correction and are two hours
behind the host; they are not timing or performance measurements.

The upstream VirtIO MSI wrapper is not Authenticode-signed. Both installer files
were fetched again from official versioned HTTPS locations and matched their
SHA-256 hashes; the serial-driver catalog validated natively. Only the serial
driver feature and QEMU guest agent were installed.

## Operations and boundaries

```sh
python3 ~/.local/share/blent-dev/windows-vm/control.py status
systemctl --user status blent-windows-vm.service
```

Shut down through Windows or `control.py shutdown`. Start the existing VM with
`open.sh`; never rerun unattended installation against its disk. The guest
checkout is `C:\src\blent`; setup and test logs are in `C:\BlentSetup`. The account
is `blentdev`; its current password is recorded in the private `credentials.json`.
Automatic login is intentionally enabled for testing.

This VM validates native Windows kernel/API and process/file ownership contracts.
It does not establish physical GPU/display-driver/USB/tablet acceptance (T522),
nor enable unfinished capture, input, camera or autostart backends.
The historical remote Linux failure was repaired with T645's isolated sysfs
coverage fixture; a new remote run is pending. T644's earlier Xvfb startup stall
remains unexplained. See the [coverage report](2026-09-26-function-coverage.md).
