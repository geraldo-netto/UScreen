# Windows regression runner evidence (T646)

The permanent package-policy test failed on Server 2022 at `5c93620`:
[run 36279734002](https://github.com/geraldo-netto/UScreen/actions/runs/36279734002).
Importing AppLocker had not initialized its native policy-model assembly.
Calling the read-only `Get-AppLockerPolicy -Local` initializes it. Existing
native identity, version-boundary, and merge assertions are unchanged.

The next [run 36279958413](https://github.com/geraldo-netto/UScreen/actions/runs/36279958413)
at `1f44272` passed every assertion but failed the step because the intentional
exit-7 fixture left a nonzero native status. `windows_vm_tools_process.ps1`
reproduced that failure in a separate Windows PowerShell process. Its retained
red/green results show exit 7 rejected before the fix and exit 0 afterward.
The suite clears its native status only after all assertions and cleanup pass.

The complete policy/tools step passed on Server 2022 in
[run 36280467116](https://github.com/geraldo-netto/UScreen/actions/runs/36280467116)
at `b6d5a08`. `server-2022-step.json` records that result while later unchanged
build/coverage steps were still running; it is not a whole-workflow success
claim. The validation branch source bytes match `source-hashes.json`.

Native process evidence came from the retained Windows 11 VM. Successful JSON
omits only PowerShell module-initialization progress; red JSON retains errors.
CI failure logs are excerpts. No test was skipped or weakened, and neither
CI regression applies machine policy. Run both normal suites as documented in
[the tooling README](../../../../scripts/dev/windows-vm/README.md).
