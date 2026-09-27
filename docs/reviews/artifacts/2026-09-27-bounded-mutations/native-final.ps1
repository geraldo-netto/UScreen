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
 $process=Join-Path $source 'scripts\mutation\process.py'
 $original=[IO.File]::ReadAllBytes($process)
 try {
  $text=[Text.Encoding]::UTF8.GetString($original).Replace("'/T', '/F'","'/F'")
  [IO.File]::WriteAllText($process,$text,(New-Object Text.UTF8Encoding($false)))
  Remove-Item (Join-Path $source 'scripts\mutation\__pycache__\process*.pyc') -ErrorAction SilentlyContinue
  $negative=Invoke-NativeCheck $python @('-m','unittest','discover','-s','scripts/tests','-p','test_mutation_runner.py','-k','windows_deadline') (Join-Path $out 'descendant-red.log')
 } finally { [IO.File]::WriteAllBytes($process,$original); Remove-Item (Join-Path $source 'scripts\mutation\__pycache__\process*.pyc') -ErrorAction SilentlyContinue }
 if($negative -eq 0){throw 'Descendant negative control unexpectedly passed'}
 $tests=Invoke-NativeCheck $python @('-m','unittest','discover','-s','scripts/tests','-p','test_*mutation*.py','-v') (Join-Path $out 'runner-tests.log')
 if($tests -ne 0){throw 'Native runner tests failed'}
 $campaign=Invoke-NativeCheck $python @('scripts/mutation/bounded.py','--profile','windows-tray','--output',(Join-Path $out 'tray')) (Join-Path $out 'campaign.log')
 @{exitcode=$campaign;runner_exitcode=$tests;negative_exitcode=$negative}|ConvertTo-Json|Set-Content (Join-Path $out 'result.json')
} catch { @{error=$_.Exception.Message;exitcode=1}|ConvertTo-Json|Set-Content (Join-Path $out 'result.json') }
