function FormatPowershell($sourcePath) {
  $SourceCode = $(Get-Content "${sourcePath}" -Raw)
  Invoke-Formatter -ScriptDefinition $SourceCode -Settings "${PSScriptRoot}/../configs/powershell-fmt-config.psd1" | Out-File -FilePath "${sourcePath}"
}

FormatPowershell "${PSScriptRoot}/../../cmd/getbridge/pwsh/getbridge.ps1"
FormatPowershell "${PSScriptRoot}/../../cmd/getbridgetype/pwsh/getbridgetype.ps1"
FormatPowershell "${PSScriptRoot}/../../cmd/setbridge/pwsh/setbridge.ps1"