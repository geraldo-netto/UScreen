# T647: run elevated in the configured VM; unavailable DNS must prevent readiness.
param(
    [Parameter(Mandatory=$true)][long]$ReferenceUnixSeconds,
    [string]$Source = (Join-Path $PSScriptRoot 'windows_vm_acceptance.ps1')
)
$ErrorActionPreference='Stop'
function Test-UnavailableDns([scriptblock]$Resolver, [string]$Expected) {
    Set-Item Function:Resolve-DnsName $Resolver
    $failure=$null
    try { & $Source -ReferenceUnixSeconds $ReferenceUnixSeconds | Out-Null }
    catch { $failure=$_.Exception.Message }
    if ($failure -ne $Expected) {
        throw "T647: unavailable DNS must prevent readiness; expected '$Expected', got '$failure'"
    }
}
Test-UnavailableDns { throw 'T647 fixture DNS unavailable' } 'T647 fixture DNS unavailable'
Test-UnavailableDns { @() } 'T647: development repository DNS resolution failed'
Write-Output 'PASS: resolver failure and empty DNS results both prevent VM readiness'
