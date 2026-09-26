# T633: read-only acceptance of the configured, retained Windows development VM.
# Run elevated with blentdev logged in. Reference time comes from the host.
param([Parameter(Mandatory=$true)][long]$ReferenceUnixSeconds)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$offset=[DateTimeOffset]::UtcNow.ToUnixTimeSeconds()-$ReferenceUnixSeconds
if ([Math]::Abs($offset) -gt 120) { throw "T633: guest clock differs from host by $offset seconds" }
$search=Get-Service WSearch
if ($search.Status -ne 'Stopped' -or $search.StartType -ne 'Disabled') { throw 'T633: indexing remains enabled' }
$policies=Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search'
if ($policies.DisableSearch -ne 1 -or $policies.ConnectedSearchUseWeb -ne 0 -or $policies.AllowCloudSearch -ne 0) {
    throw 'T633: Search policy mismatch'
}
$user=Get-LocalUser blentdev
$userSearch=Get-ItemProperty ("Registry::HKEY_USERS\"+$user.SID.Value+'\Software\Microsoft\Windows\CurrentVersion\Search')
if ($userSearch.SearchboxTaskbarMode -ne 0) { throw 'T633: user Search taskbar entry remains enabled' }
$winlogon=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
if ($winlogon.AutoAdminLogon -ne '1' -or $winlogon.DefaultUserName -ne 'blentdev' -or
    'DefaultPassword' -notin $winlogon.PSObject.Properties.Name) {
    throw 'T633: maintainer-approved test VM automatic login is not configured'
}
$uac=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System'
if ($uac.EnableLUA -ne 1 -or $uac.ConsentPromptBehaviorAdmin -ne 0) { throw 'T633: approved VM elevation policy mismatch' }
$tpm=Get-Tpm
$secureBoot=Confirm-SecureBootUEFI
if (-not $tpm.TpmPresent -or -not $tpm.TpmReady -or -not $secureBoot) { throw 'T633: TPM/Secure Boot unavailable' }
$identity=Get-Service AppIDSvc
if ($identity.Status -ne 'Running') { throw 'T633: application identity enforcement unavailable' }
$xml=Join-Path $env:TEMP ('Blent acceptance '+[guid]::NewGuid().ToString('N')+'.xml')
try {
    Get-AppLockerPolicy -Effective -Xml | Set-Content $xml
    $package=Get-AppxPackage -AllUsers Microsoft.XboxGameCallableUI
    if (@($package).Count -ne 1) { throw 'T633: protected Xbox package must remain installed for servicing' }
    $decision=$package | Test-AppLockerPolicy -XmlPolicy $xml -User blentdev
    if ($decision.PolicyDecision.ToString() -ne 'Denied') { throw 'T633: protected Xbox launch policy is not enforced' }
    $shell=Get-AppxPackage -AllUsers Microsoft.Windows.ShellExperienceHost
    if (@($shell).Count -ne 1) { throw 'T633: protected shell package missing' }
    $decision=$shell | Test-AppLockerPolicy -XmlPolicy $xml -User blentdev
    if ($decision.PolicyDecision.ToString() -ne 'Allowed') { throw 'T633: shell launch policy is not preserved' }
} finally { Remove-Item $xml -ErrorAction SilentlyContinue }
$os=Get-CimInstance Win32_OperatingSystem
@{clock_offset_seconds=$offset; os=$os.Caption; build=$os.BuildNumber; architecture=$os.OSArchitecture;
  tpm_present=$tpm.TpmPresent; tpm_ready=$tpm.TpmReady; secure_boot=$secureBoot;
  search_disabled=$true; autologon_enabled=$true; elevation_policy_verified=$true;
  xbox_policy='Denied'; shell_policy='Allowed'; checked_utc=[DateTime]::UtcNow.ToString('o')} | ConvertTo-Json
