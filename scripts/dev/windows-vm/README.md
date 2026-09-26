# Dedicated Windows development VM

`minimal.ps1` applies the maintainer's accepted minimal Windows development
configuration. Run it elevated only in the dedicated development VM. It removes
removable consumer apps for all users, disables Search/indexing and consumer
features, blocks protected Xbox/Copilot packaged entry points, and permits
administrator elevation without prompts as explicitly requested. UAC remains
enabled; Windows servicing and protected shell packages remain installed.

The script writes `C:\BlentSetup\minimal-state.json`; that directory must exist.
Verify native state after execution. Preparation or a registry write alone does
not establish that a policy took effect. VM disk, TPM, firmware, credentials and
installation media stay outside Git and must be retained.

Run the permanent native regressions in Windows PowerShell without applying
machine policies or removing apps:

```powershell
./scripts/tests/windows_vm_policy.ps1
./scripts/tests/windows_vm_tools_process.ps1
```

The policy test extracts isolated helpers, uses a temporary owned HKCU key,
and evaluates package identities with Windows' native AppLocker evaluator.
It also verifies that replacing owned rule IDs preserves foreign rules and
collections. Exact package names are deliberate: the native evaluator did not
match the previous partial `Microsoft.Xbox*` wildcard. Policy propagation is
asynchronous; verify effective state and actual activation after applying it.

`run-native.ps1` supplies `Invoke-NativeCheck` for bootstrap Cargo commands.
It logs native stderr without treating successful diagnostics as terminating
PowerShell errors, and returns the actual process exit code. Windows CI runs both
PowerShell regressions. Neither test applies the VM's administrator policy.
The process wrapper also verifies that an expected native failure inside the
test does not leak a failing exit status after all assertions pass (T646).
AppLocker types are initialized through a read-only native cmdlet on Server 2022.

`guest_status.py` supplies `read_file(request, path)` to the private QEMU guest
control adapter. Pass its serialized QGA request function. A bounded PowerShell
process owns and closes each status read, with write/delete sharing enabled.
Do not use persistent `guest-file-open` handles for monitoring: disconnecting or
killing the host client can leave those handles open inside the guest agent.
The retained regression covers lost replies, failed/truncated/oversized results,
Unicode, invalid paths and timeouts without relaunching a process:

```sh
python3 -m unittest discover -s scripts/tests -p test_windows_vm_status.py
```

These helpers are nonessential development tooling under the maintainer's
existing coverage exemption; they are not Blent installers or runtime backends.

Final VM acceptance is a separate native suite because it requires the actual
VM configuration, elevation, and a logged-in `blentdev` profile. Run
`scripts/tests/windows_vm_acceptance.ps1 -ReferenceUnixSeconds <host-epoch>`
through guest control using a fresh Unix timestamp from the Linux host. It checks
clock alignment, Search/indexing, automatic login, UAC, TPM/Secure Boot and native
package decisions, plus GitHub DNS resolution. DNS failure or empty address
results prevent a ready result (T647). The permanent negative tests run as
`scripts/tests/windows_vm_dns.ps1 -ReferenceUnixSeconds <host-epoch>` in the
same configured VM; follow them with normal acceptance using a fresh timestamp.
They replace resolution only inside the test scope and do not change DNS settings.
The private QEMU launcher must use `-rtc base=localtime` for
this Windows configuration; the retained clock regression failed with the
default UTC RTC. Repeat acceptance after a normal stop/start of the existing VM.
Do not run this suite on a general CI runner or apply the VM policies there.
