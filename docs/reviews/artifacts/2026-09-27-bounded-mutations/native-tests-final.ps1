$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$out=Join-Path $env:LOCALAPPDATA 'Temp\blent-t652-bounded-final'
[IO.Directory]::CreateDirectory($out)|Out-Null
$source=Join-Path $env:LOCALAPPDATA 'Temp\blent-t531\source'
$env:CARGO_HOME=Join-Path $env:USERPROFILE '.cargo'; $env:RUSTUP_HOME=Join-Path $env:USERPROFILE '.rustup'
$env:PATH="$env:CARGO_HOME\bin;C:\Tools\Git\cmd;C:\Tools\FFmpeg;"+$env:PATH
$env:CARGO_BUILD_JOBS='4'
. C:\BlentSetup\run-native.ps1
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 $principal=New-Object Security.Principal.WindowsPrincipal($identity)
 if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {throw 'Ordinary user required'}
 Expand-Archive C:\BlentSetup\t652-tools-final.zip $source -Force
 Set-Location $source
 & C:\Tools\Git\cmd\git.exe init -q
 if($LASTEXITCODE -ne 0){throw 'Private git inventory failed'}
 $python=Join-Path $env:LOCALAPPDATA 'Temp\blent-mutation-recovery\python\python.exe'
 $pythonRoot=Split-Path $python
 @('python312.zip','.',"$source\scripts\mutation",'import site') | Set-Content (Join-Path $pythonRoot 'python312._pth') -Encoding ASCII
 @{user=$identity.Name;elevated=$false;session=(Get-Process -Id $PID).SessionId}|ConvertTo-Json|Set-Content (Join-Path $out 'identity.json')
 Copy-Item C:\BlentSetup\t652-bounded-final.py (Join-Path $source 'scripts\mutation\bounded.py') -Force
 Copy-Item C:\BlentSetup\t652-isolation-final.py (Join-Path $source 'scripts\mutation\isolation.py') -Force
 Copy-Item C:\BlentSetup\t652-tests-final.py (Join-Path $source 'scripts\tests\test_bounded_mutation.py') -Force
 $tests=Invoke-NativeCheck $python @('-m','unittest','discover','-s','scripts/tests','-p','test_*mutation*.py','-v') (Join-Path $out 'runner-tests-final.log')
 if($tests -ne 0){throw 'Native runner tests failed'}
 @{exitcode=$tests}|ConvertTo-Json|Set-Content (Join-Path $out 'validation-tests-final.json')
} catch { @{error=$_.Exception.Message;exitcode=1}|ConvertTo-Json|Set-Content (Join-Path $out 'validation-tests-final.json') }
