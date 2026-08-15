$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot "release-build-config.ps1")

$installer = "C:\Users\sunda\Documents\LLMservice\ccswitchmulti-qwen38-original-fix-7fbcdd01\windows\installer\CCSwitchMulti_3.19.1-31_x64-setup.exe"
$rawExecutable = Join-Path $repoRoot "src-tauri\target\release\cc-switch.exe"
$installedExecutable = "C:\Users\sunda\AppData\Local\CCSwitchMulti\cc-switch.exe"
$installDirectory = Split-Path -Parent $installedExecutable
$uninstallExecutable = Join-Path $installDirectory "uninstall.exe"
$listener = Get-NetTCPConnection -State Listen -LocalPort 15721 | Select-Object -First 1
if (-not $listener) { throw "CCSwitchMulti is not listening on port 15721" }

$transactionId = "ccsm-20260816-qwen38-original-7fbcdd01"
$backupRoot = Join-Path "C:\Users\sunda\AppData\Local\CCSwitchMultiTransactionBackups" $transactionId
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null
$resultPath = Join-Path $backupRoot "transaction-result.json"
$stderrPath = "$resultPath.stderr"

$arguments = @(
    "-NoProfile", "-ExecutionPolicy", "Bypass",
    "-File", (Join-Path $PSScriptRoot "install-ccswitchmulti-transaction.ps1"),
    "-InstallerPath", $installer,
    "-ExpectedInstallerHash", (Get-ReleaseFileSha256 -Path $installer),
    "-ExpectedCurrentVersion", (Get-Item -LiteralPath $installedExecutable).VersionInfo.ProductVersion,
    "-ExpectedCurrentHash", (Get-ReleaseFileSha256 -Path $installedExecutable),
    "-ExpectedInstalledVersion", "3.19.1-31",
    "-ExpectedInstalledHash", (Get-TauriNsisInstalledExeSha256 -Path $rawExecutable),
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
