$ErrorActionPreference = 'Stop'
function Set-Dword($Path, $Name, $Value) {
    if (-not (Test-Path $Path)) { New-Item -Path $Path -Force | Out-Null }
    New-ItemProperty -Path $Path -Name $Name -Value $Value -PropertyType DWord -Force | Out-Null
}
function Merge-OwnedPolicy($CurrentXml, $DesiredXml) {
    [xml]$current=$CurrentXml
    [xml]$desired=$DesiredXml
    foreach ($incoming in $desired.AppLockerPolicy.RuleCollection) {
        $collection=$current.SelectSingleNode('//RuleCollection[@Type="'+$incoming.Type+'"]')
        if ($null -eq $collection) {
            $current.AppLockerPolicy.AppendChild($current.ImportNode($incoming,$true)) | Out-Null
        } else {
            $collection.SetAttribute('EnforcementMode',$incoming.EnforcementMode)
            foreach ($rule in $incoming.ChildNodes) {
                foreach ($old in @($collection.SelectNodes('*[@Id="'+$rule.Id+'"]'))) {
                    $collection.RemoveChild($old) | Out-Null
                }
                $collection.AppendChild($current.ImportNode($rule,$true)) | Out-Null
            }
        }
    }
    return $current.OuterXml
}
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search' 'DisableSearch' 1
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search' 'ConnectedSearchUseWeb' 0
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search' 'AllowCloudSearch' 0
Stop-Service WSearch -Force -ErrorAction SilentlyContinue
Set-Service WSearch -StartupType Disabled
Set-Dword 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Search' 'SearchboxTaskbarMode' 0
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Dsh' 'AllowNewsAndInterests' 0
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent' 'DisableWindowsConsumerFeatures' 1
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\CloudContent' 'DisableSoftLanding' 1
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\GameDVR' 'AllowGameDVR' 0
Set-Dword 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsAI' 'DisableAIDataAnalysis' 1
$pattern = '^(Microsoft\.(Xbox.*|GamingApp|GamingServices|MicrosoftSolitaireCollection|Copilot|MicrosoftOfficeHub|BingNews|BingWeather|Teams|OutlookForWindows|Todos|PowerAutomateDesktop|YourPhone|GetHelp|Getstarted|MicrosoftStickyNotes|WindowsFeedbackHub|WindowsMaps|ZuneMusic|ZuneVideo)|MSTeams|Clipchamp\.Clipchamp|MicrosoftWindows\.Client\.WebExperience)$'
$removed = @()
Get-AppxProvisionedPackage -Online | Where-Object DisplayName -Match $pattern | ForEach-Object {
    Remove-AppxProvisionedPackage -Online -PackageName $_.PackageName | Out-Null
    $removed += $_.DisplayName
}
Get-AppxPackage -AllUsers | Where-Object { $_.Name -match $pattern -and -not $_.NonRemovable } | ForEach-Object {
    Remove-AppxPackage -AllUsers -Package $_.PackageFullName
    $removed += $_.Name
}
$rules = @'
<AppLockerPolicy Version="1"><RuleCollection Type="Appx" EnforcementMode="Enabled">
<FilePublisherRule Id="288ee930-9f2c-48cc-a3d8-8ee1cdb02001" Name="Allow other signed packaged apps" Description="Preserve Windows shell components" UserOrGroupSid="S-1-1-0" Action="Allow"><Conditions><FilePublisherCondition PublisherName="*" ProductName="*" BinaryName="*"><BinaryVersionRange LowSection="0.0.0.0" HighSection="*" /></FilePublisherCondition></Conditions></FilePublisherRule>
<FilePublisherRule Id="288ee930-9f2c-48cc-a3d8-8ee1cdb02002" Name="Block consumer Copilot" Description="Minimal development VM" UserOrGroupSid="S-1-1-0" Action="Deny"><Conditions><FilePublisherCondition PublisherName="CN=MICROSOFT CORPORATION, O=MICROSOFT CORPORATION, L=REDMOND, S=WASHINGTON, C=US" ProductName="Microsoft.Copilot" BinaryName="*"><BinaryVersionRange LowSection="0.0.0.0" HighSection="*" /></FilePublisherCondition></Conditions></FilePublisherRule>
<FilePublisherRule Id="288ee930-9f2c-48cc-a3d8-8ee1cdb02003" Name="Block Xbox Game Bar" Description="Minimal development VM" UserOrGroupSid="S-1-1-0" Action="Deny"><Conditions><FilePublisherCondition PublisherName="CN=MICROSOFT CORPORATION, O=MICROSOFT CORPORATION, L=REDMOND, S=WASHINGTON, C=US" ProductName="Microsoft.XboxGamingOverlay" BinaryName="*"><BinaryVersionRange LowSection="0.0.0.0" HighSection="*" /></FilePublisherCondition></Conditions></FilePublisherRule>
<FilePublisherRule Id="288ee930-9f2c-48cc-a3d8-8ee1cdb02004" Name="Block protected Xbox entry points" Description="Preserve package servicing, deny launch" UserOrGroupSid="S-1-1-0" Action="Deny"><Conditions><FilePublisherCondition PublisherName="CN=MICROSOFT WINDOWS, O=MICROSOFT CORPORATION, L=REDMOND, S=WASHINGTON, C=US" ProductName="Microsoft.XboxGameCallableUI" BinaryName="*"><BinaryVersionRange LowSection="0.0.0.0" HighSection="*" /></FilePublisherCondition></Conditions></FilePublisherRule>
</RuleCollection></AppLockerPolicy>
'@
$current=Get-AppLockerPolicy -Local -Xml
Merge-OwnedPolicy $current $rules | Set-Content C:\BlentSetup\minimal-apps.xml
Set-AppLockerPolicy -XmlPolicy C:\BlentSetup\minimal-apps.xml
& sc.exe config AppIDSvc start= auto | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Application Identity service configuration failed' }
Start-Service AppIDSvc
$remaining = @(Get-AppxPackage -AllUsers | Where-Object Name -Match $pattern | Select-Object Name,NonRemovable)
@{removed=$removed; remaining=$remaining; search=(Get-Service WSearch | Select-Object Status,StartType); policy=(Get-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Windows Search' | Select-Object DisableSearch,ConnectedSearchUseWeb,AllowCloudSearch)} | ConvertTo-Json -Depth 5 | Set-Content C:\BlentSetup\minimal-state.json
if ((Get-Service WSearch).StartType -ne 'Disabled') { throw 'Search service still enabled' }
if ($remaining | Where-Object { -not $_.NonRemovable }) { throw 'Unwanted removable apps remain' }

# Maintainer authorized unattended administrator elevation in this VM.
Set-Dword 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' 'ConsentPromptBehaviorAdmin' 0
