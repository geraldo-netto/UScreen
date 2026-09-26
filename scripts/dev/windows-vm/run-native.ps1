function Invoke-NativeCheck($File, $Arguments, $Log) {
    $command=Get-Command $File -CommandType Application -ErrorAction Stop
    # Windows PowerShell 5.1 wraps redirected native stderr as ErrorRecords.
    # Their presence says nothing about the process exit status.
    $ErrorActionPreference='Continue'
    $PSNativeCommandUseErrorActionPreference=$false
    $global:LASTEXITCODE=$null
    & $command.Source @Arguments *> $Log
    if ($null -eq $global:LASTEXITCODE) { throw 'Native command did not report an exit code' }
    return $global:LASTEXITCODE
}
