$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$out=Join-Path $env:LOCALAPPDATA 'Temp\blent-t531'
[IO.Directory]::CreateDirectory($out)|Out-Null
$env:CARGO_HOME=Join-Path $env:USERPROFILE '.cargo'; $env:RUSTUP_HOME=Join-Path $env:USERPROFILE '.rustup'
$env:PATH="$env:CARGO_HOME\bin;C:\Tools\Git\cmd;C:\Tools\FFmpeg;"+$env:PATH
$env:CARGO_BUILD_JOBS='4'
. C:\BlentSetup\run-native.ps1
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 $principal=New-Object Security.Principal.WindowsPrincipal($identity)
 if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {throw 'Ordinary user required'}
 $source=Join-Path $out 'source'
 Expand-Archive C:\BlentSetup\t531-final-overlay.zip $source -Force
 Set-Location $source
 @{user=$identity.Name;elevated=$false;session=(Get-Process -Id $PID).SessionId;explorer=@(Get-Process explorer | Select-Object Id,SessionId)}|ConvertTo-Json -Depth 4 | Set-Content (Join-Path $out 'identity.json')
 $hashes=@{}; foreach ($folder in @('common/src','host/src','gui/src')) { Get-ChildItem $folder -Recurse -Filter *.rs | ForEach-Object { $relative=$_.FullName.Substring($source.Length+1).Replace('\','/'); $hashes[$relative]=(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower() } }
 $hashes|ConvertTo-Json -Depth 3 | Set-Content (Join-Path $out 'native-sources.json')
 $env:CARGO_LLVM_COV_TARGET_DIR=Join-Path $out 'coverage-target-v3'
 $code=Invoke-NativeCheck "$env:CARGO_HOME\bin\cargo.exe" @('llvm-cov','--locked','-p','blent','--lib','--test','windows_usb','--test','windows_cli','--test','windows_lifecycle','--remap-path-prefix','--lcov','--output-path',(Join-Path $out 'windows-v3.lcov')) (Join-Path $out 'tests-v3.log')
 @{exitcode=$code}|ConvertTo-Json | Set-Content (Join-Path $out 'result-v3.json')
} catch { @{error=$_.Exception.Message;exitcode=1}|ConvertTo-Json | Set-Content (Join-Path $out 'result-v3.json') }
