# T646: expected child failures must not leak into the caller's CI exit status.
param([string]$TestScript = (Join-Path $PSScriptRoot 'windows_vm_tools.ps1'))
$ErrorActionPreference='Stop'
$quoted=$TestScript.Replace("'","''")
$command="`$ErrorActionPreference='Stop'; & '$quoted'; if (Test-Path variable:\LASTEXITCODE) { exit `$LASTEXITCODE }"
$encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($command))
& "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded
if ($LASTEXITCODE -ne 0) { throw "T646: successful native regression suite leaked exit code $LASTEXITCODE" }
Write-Output 'PASS: successful regression process exits zero after expected native failures'
