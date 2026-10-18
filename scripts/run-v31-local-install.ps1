param(
    [Parameter(Mandatory = $true)][string]$ProductionManifestPath,
    [Parameter(Mandatory = $true)][string]$ExpectedSourceCommit
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot "production-artifacts.ps1")

$production = Assert-ProductionManifest -ManifestPath $ProductionManifestPath -ExpectedSourceCommit $ExpectedSourceCommit -ExpectedVersion '3.19.1-31'
$installer = $production.InstallerPath
$installedExecutable = "C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe"
$installDirectory = Split-Path -Parent $installedExecutable
$uninstallExecutable = Join-Path $installDirectory "uninstall.exe"
$listener = Get-NetTCPConnection -State Listen -LocalPort 15721 | Select-Object -First 1
if (-not $listener) { throw "CCSwitchMulti is not listening on port 15721" }

$transactionId = "ccsm-20260816-qwen38-original-toml-root-7e6515db-r3"
$backupRoot = Join-Path "C:\Users\sunda\AppData\Local\CCSwitchMultiTransactionBackups" $transactionId
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null
$resultPath = Join-Path $backupRoot "transaction-result.json"
$stderrPath = "$resultPath.stderr"

$arguments = @(
    "-NoProfile", "-ExecutionPolicy", "Bypass",
    "-File", (Join-Path $PSScriptRoot "install-ccswitchmulti-transaction.ps1"),
    "-InstallerPath", $installer,
    "-ExpectedInstallerHash", $production.Manifest.artifacts.installer.sha256,
    "-ProductionManifestPath", $ProductionManifestPath,
    "-ExpectedSourceCommit", $ExpectedSourceCommit,
    "-ExpectedCurrentVersion", (Get-Item -LiteralPath $installedExecutable).VersionInfo.ProductVersion,
    "-ExpectedCurrentHash", (Get-ReleaseFileSha256 -Path $installedExecutable),
    "-ExpectedInstalledVersion", "3.19.1-31",
    "-ExpectedInstalledHash", $production.Manifest.artifacts.rawExecutable.installedSha256,
    "-CurrentPid", [string]$listener.OwningProcess,
    "-InstalledExecutable", $installedExecutable,
    "-InstallDirectory", $installDirectory,
    "-UninstallExecutable", $uninstallExecutable,
    "-ConfigPath", "C:\Users\sunda\.cc-switch",
    "-RegistryKey", "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti",
    "-Port", "15721",
    "-HealthUri", "http://127.0.0.1:15721/health",
    "-TimeoutSeconds", "120",
    "-BackupRoot", $backupRoot
)

$process = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList $arguments `
    -RedirectStandardOutput $resultPath -RedirectStandardError $stderrPath

[pscustomobject]@{
    TransactionId = $transactionId
    ProcessId = $process.Id
    ResultPath = $resultPath
} | ConvertTo-Json -Compress
