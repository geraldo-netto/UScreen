param([string]$Source = (Join-Path $PSScriptRoot '..\dev\windows-vm\minimal.ps1'))
$ErrorActionPreference='Stop'
$ast=[Management.Automation.Language.Parser]::ParseFile($Source,[ref]$null,[ref]$null)
$definition=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Set-Dword'},$true)
. ([ScriptBlock]::Create($definition.Extent.Text))
$key='HKCU:\Software\BlentSetupTests\'+[guid]::NewGuid().ToString('N')
try {
    Set-Dword $key 'First' 1
    Set-Dword $key 'Second' 0
    $actual=Get-ItemProperty $key
    if ($actual.First -ne 1 -or $actual.Second -ne 0) { throw 'T633: setting a policy destroyed its sibling value' }
    Set-Dword $key 'First' 0
    if ((Get-ItemProperty $key).Second -ne 0) { throw 'T633: updating a policy destroyed its sibling value' }
    Write-Output 'PASS: sibling policy values survive creation and update'
} finally { Remove-Item $key -Recurse -Force -ErrorAction SilentlyContinue }

$assignment=$ast.Find({param($node) $node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$rules'},$true)
[xml]$policy=$assignment.Right.Expression.Value
$denials=@($policy.AppLockerPolicy.RuleCollection.FilePublisherRule | Where-Object Action -eq 'Deny')
$corporation='CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US'
$windows='CN=Microsoft Windows, O=Microsoft Corporation, L=Redmond, S=Washington, C=US'
$cases=@(
    @{Name='Microsoft.XboxGameCallableUI'; Publisher=$windows; Denied=$true},
    @{Name='Microsoft.XboxGamingOverlay'; Publisher=$corporation; Denied=$true},
    @{Name='Microsoft.Copilot'; Publisher=$corporation; Denied=$true},
    @{Name='Microsoft.Windows.ShellExperienceHost'; Publisher=$windows; Denied=$false},
    @{Name='Microsoft.WindowsTerminal'; Publisher=$corporation; Denied=$false}
)
foreach ($case in $cases) {
    $matching=@($denials | Where-Object {
        $condition=$_.Conditions.FilePublisherCondition
        $case.Name -like $condition.ProductName -and $case.Publisher -like $condition.PublisherName
    })
    if (($matching.Count -gt 0) -ne $case.Denied) {
        throw "T633: packaged app policy mismatch for $($case.Name)"
    }
}
Write-Output 'PASS: packaged app denials cover actual publishers and preserve shell/tools'

# T633: exercise Windows' actual package evaluator. PowerShell -like accepts
# partial wildcards that this native AppLocker path does not match.
Import-Module AppLocker
$nativePolicy=[Microsoft.Security.ApplicationId.PolicyManagement.PolicyModel.AppLockerPolicy]::FromXml($policy.OuterXml)
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
foreach ($case in $cases) {
    foreach ($version in @('0.0.0.0','1000.25128.1000.0','65535.65535.65535.65535')) {
        $fileVersion=[Microsoft.Security.ApplicationId.PolicyManagement.PolicyModel.FileVersion]::new($version)
        $publisher=[Microsoft.Security.ApplicationId.PolicyManagement.PolicyModel.FilePublisher]::new($case.Publisher,$case.Name,'APPX',$fileVersion)
        $path=[Microsoft.Security.ApplicationId.PolicyManagement.PolicyModel.FilePath]::new($case.Name+'.appx')
        $information=[Microsoft.Security.ApplicationId.PolicyManagement.PolicyModel.FileInformation]::new($path,$publisher,$null,$true)
        $result=[Microsoft.Security.ApplicationId.PolicyManagement.PolicyManager]::IsPackageAllowed($nativePolicy,$information,$sid)
        $expected=if ($case.Denied) {'Denied'} else {'Allowed'}
        if ($result.PolicyDecision.ToString() -ne $expected) {
            throw "T633: native package decision for $($case.Name) $version is $($result.PolicyDecision); expected $expected"
        }
    }
}
Write-Output 'PASS: native AppLocker package decisions and version boundaries'

# T633: native merge keeps the old rule when its ID is reused. Updating owned
# rules must retain foreign rules and other rule collections unchanged.
$merge=$ast.Find({param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Merge-OwnedPolicy'},$true)
. ([ScriptBlock]::Create($merge.Extent.Text))
[xml]$legacy=$policy.OuterXml
$stale=$legacy.AppLockerPolicy.RuleCollection.FilePublisherRule | Where-Object Id -eq '288ee930-9f2c-48cc-a3d8-8ee1cdb02004'
$stale.Conditions.FilePublisherCondition.ProductName='Microsoft.Xbox*'
$foreign=$stale.CloneNode($true)
$foreign.Id='9972cc27-6ac3-4b40-b8dd-e299c1a44444'
$foreign.Conditions.FilePublisherCondition.ProductName='Foreign.Package'
$legacy.AppLockerPolicy.RuleCollection.AppendChild($foreign) | Out-Null
$other=$legacy.CreateElement('RuleCollection')
$other.SetAttribute('Type','Exe')
$other.SetAttribute('EnforcementMode','AuditOnly')
$legacy.AppLockerPolicy.AppendChild($other) | Out-Null
[xml]$merged=Merge-OwnedPolicy $legacy.OuterXml $policy.OuterXml
$owned=$merged.SelectSingleNode('//*[@Id="288ee930-9f2c-48cc-a3d8-8ee1cdb02004"]')
if ($owned.Conditions.FilePublisherCondition.ProductName -ne 'Microsoft.XboxGameCallableUI') {
    throw 'T633: applying updated policy retained the obsolete same-ID wildcard'
}
if ($merged.SelectSingleNode('//*[@Id="9972cc27-6ac3-4b40-b8dd-e299c1a44444"]').OuterXml -ne $foreign.OuterXml) {
    throw 'T633: policy update changed an unrelated packaged-app rule'
}
if ($merged.SelectSingleNode('//RuleCollection[@Type="Exe"]').OuterXml -ne $other.OuterXml) {
    throw 'T633: policy update changed an unrelated rule collection'
}
if ((Merge-OwnedPolicy $merged.OuterXml $policy.OuterXml) -ne $merged.OuterXml) {
    throw 'T633: repeated policy update is not idempotent'
}
Write-Output 'PASS: owned policy updates preserve foreign rules and are idempotent'
