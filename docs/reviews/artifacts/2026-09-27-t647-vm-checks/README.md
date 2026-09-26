# Repository DNS readiness evidence (T647)

The retained VM temporarily could not resolve `github.com` through its DHCP
resolver, so `git pull` failed despite earlier native acceptance succeeding.
Resolution recovered without a DNS configuration change. This change detects
that missing prerequisite during acceptance; it does not claim to fix the
transient upstream resolver failure.

`windows_vm_dns.ps1` overrides DNS resolution within its test scope. With the
original acceptance script from `5c93620`, simulated resolver failure still
returned readiness, so the permanent regression failed. The updated acceptance
script rejects both a resolver error and an empty answer. The same regression
then passed. Native JSON red/green results are retained here; successful results
omit only PowerShell module-initialization progress. Red JSON retains its errors.

Normal acceptance subsequently passed with real DNS, automatic login, a clock
offset of -1 second, TPM/Secure Boot, Search and native package policies. The
clean guest checkout updated from `9330d93` to `5c93620` through ordinary Git
with its original DHCP settings, as recorded in `t647-checkout.json`.
`source-hashes.json` identifies the tested acceptance and negative-test bytes.

Both suites require the configured Windows VM and elevation; they cannot run
on an ordinary CI runner. Their normal commands are in
[the tooling README](../../../../scripts/dev/windows-vm/README.md).
