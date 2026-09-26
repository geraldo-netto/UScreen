param([string]$Source = (Join-Path $PSScriptRoot '..\dev\windows-vm\run-native.ps1'))
$ErrorActionPreference='Stop'
. $Source
$directory=Join-Path $env:TEMP ('Blent tools test '+[guid]::NewGuid().ToString('N'))
New-Item $directory -ItemType Directory | Out-Null
try {
    foreach ($code in @(0,7)) {
        $log=Join-Path $directory "native-$code.log"
        $result=Invoke-NativeCheck $env:ComSpec @('/d','/c',"echo T633 stderr 1>&2 & exit /b $code") $log
        if ($result -ne $code) { throw "T633: native exit code changed from $code to $result" }
        if ([IO.File]::ReadAllText($log) -notmatch 'T633 stderr') { throw 'T633: native stderr was lost' }
    }
    Write-Output 'PASS: native stderr is logged and exit codes determine success'
} finally { Remove-Item $directory -Recurse -Force }
# T646: the expected failure above is fixture data, not this suite's result.
$global:LASTEXITCODE=0
