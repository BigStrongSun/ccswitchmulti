#requires -Version 5.1
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSEdition -ne 'Desktop') {
    throw 'Run this suite with Windows PowerShell 5.1 (powershell.exe), matching the release/install entry points.'
}
Import-Module Pester -RequiredVersion 3.4.0 -Force -ErrorAction Stop
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    & node --test scripts/tests/production-assets.test.cjs
    if ($LASTEXITCODE -ne 0) { throw 'Production frontend tests failed' }
    $result = Invoke-Pester -Script @(
        'scripts/tests/release-build-config.Tests.ps1',
        'scripts/tests/production-artifacts.Tests.ps1',
        'scripts/tests/install-ccswitchmulti-transaction.Tests.ps1'
    ) -PassThru
    if ($result.FailedCount -gt 0 -or $result.TotalCount -eq 0) {
        throw "Production release tests failed: $($result.FailedCount)/$($result.TotalCount)"
    }
} finally {
    Pop-Location
}
