[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallerPath,
    [Parameter(Mandatory = $true)][string]$ExpectedInstallerHash,
    [Parameter(Mandatory = $true)][string]$ExpectedInstalledVersion,
    [Parameter(Mandatory = $true)][string]$ExpectedInstalledHash,
    [string]$InstalledExecutable = "$env:LOCALAPPDATA\CCSwitchMulti\cc-switch.exe",
    [string]$ConfigPath = "$env:USERPROFILE\.cc-switch",
    [string]$RegistryKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti",
    [int]$Port = 15721,
    [string]$HealthUri = "http://127.0.0.1:15721/health",
    [int]$TimeoutSeconds = 120,
    [string]$BackupParent = "$env:LOCALAPPDATA\CCSwitchMultiTransactionBackups",
    [string]$MaintenanceMarker = "$env:LOCALAPPDATA\CCSwitchMultiGuardian\maintenance.lock"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "ccswitchmulti-guardian-core.ps1")

$transactionScript = Join-Path $PSScriptRoot "install-ccswitchmulti-transaction.ps1"
$installDirectory = Split-Path -Parent $InstalledExecutable
$uninstallExecutable = Join-Path $installDirectory "uninstall.exe"
foreach ($requiredFile in @($transactionScript, $InstallerPath, $InstalledExecutable, $uninstallExecutable)) {
    if (-not (Test-Path -LiteralPath $requiredFile -PathType Leaf)) { throw "required file missing: $requiredFile" }
}
if (-not [string]::Equals(
        (Get-CcsmGuardianFileSha256 -LiteralPath $InstallerPath),
        $ExpectedInstallerHash,
        [System.StringComparison]::OrdinalIgnoreCase
    )) {
    throw "installer hash mismatch"
}

$listener = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction Stop |
    Where-Object { $_.LocalAddress -in @("127.0.0.1", "0.0.0.0", "::") } |
    Select-Object -First 1
if ($null -eq $listener) { throw "CCSwitchMulti listener missing" }
$listenerIdentity = Get-CcsmGuardianProcessIdentity -ProcessId ([int]$listener.OwningProcess)
if (-not (Test-CcsmGuardianSamePath -Left ([string]$listenerIdentity.Path) -Right $InstalledExecutable)) {
    throw "listener executable path mismatch"
}
$health = Invoke-WebRequest -UseBasicParsing -Uri $HealthUri -TimeoutSec 5
if ([int]$health.StatusCode -ne 200) { throw "CCSwitchMulti preflight health failed" }

$currentItem = Get-Item -LiteralPath $InstalledExecutable
$currentVersion = [string]$currentItem.VersionInfo.ProductVersion
$currentHash = Get-CcsmGuardianFileSha256 -LiteralPath $InstalledExecutable
$transactionId = "ccsm-$(Get-Date -Format 'yyyyMMdd-HHmmss')-$([guid]::NewGuid().ToString('N'))"
$backupRoot = Join-Path $BackupParent $transactionId
New-Item -ItemType Directory -Force -Path $backupRoot | Out-Null
$transactionResultPath = Join-Path $backupRoot "transaction-result.json"
$transactionStderrPath = Join-Path $backupRoot "transaction-result.stderr.log"
$wrapperResultPath = Join-Path $backupRoot "upgrade-wrapper-result.json"

$leaseDurationSeconds = [Math]::Max(600, ($TimeoutSeconds * 4) + 120)
Invoke-CcsmMaintenanceLeaseScope -MarkerPath $MaintenanceMarker -Purpose "local-upgrade:$transactionId" `
    -DurationSeconds $leaseDurationSeconds -Action {
    param($LeaseId)
    $arguments = @(
        "-NoProfile", "-ExecutionPolicy", "Bypass",
        "-File", $transactionScript,
        "-InstallerPath", $InstallerPath,
        "-ExpectedInstallerHash", $ExpectedInstallerHash,
        "-ExpectedCurrentVersion", $currentVersion,
        "-ExpectedCurrentHash", $currentHash,
        "-ExpectedInstalledVersion", $ExpectedInstalledVersion,
        "-ExpectedInstalledHash", $ExpectedInstalledHash,
        "-CurrentPid", [string]$listener.OwningProcess,
        "-InstalledExecutable", $InstalledExecutable,
        "-InstallDirectory", $installDirectory,
        "-UninstallExecutable", $uninstallExecutable,
        "-ConfigPath", $ConfigPath,
        "-RegistryKey", $RegistryKey,
        "-Port", [string]$Port,
        "-HealthUri", $HealthUri,
        "-TimeoutSeconds", [string]$TimeoutSeconds,
        "-BackupRoot", $backupRoot
    )
    $process = Start-Process powershell.exe -WindowStyle Hidden -PassThru -ArgumentList $arguments `
        -RedirectStandardOutput $transactionResultPath -RedirectStandardError $transactionStderrPath
    $exitCode = Wait-CcsmOwnedProcessExit -Process $process
    if ($exitCode -ne 0) { throw "install transaction failed with exit code $exitCode" }

    $transactionResult = [System.IO.File]::ReadAllText($transactionResultPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json
    if (-not [string]::Equals([string]$transactionResult.Status, "Success", [System.StringComparison]::Ordinal)) {
        throw "install transaction did not report success"
    }
    $wrapperResult = [ordered]@{
        Status = "Success"
        TransactionId = $transactionId
        BackupRoot = $backupRoot
        OldPid = [int]$listener.OwningProcess
        NewPid = [int]$transactionResult.NewPid
        OldVersion = $currentVersion
        OldHash = $currentHash
        NewVersion = $ExpectedInstalledVersion
        NewHash = $ExpectedInstalledHash
        InstallerHash = $ExpectedInstallerHash
        TransactionResultPath = $transactionResultPath
    }
    [System.IO.File]::WriteAllText(
        $wrapperResultPath,
        ($wrapperResult | ConvertTo-Json -Depth 5),
        [System.Text.UTF8Encoding]::new($false)
    )
    $wrapperResult | ConvertTo-Json -Compress
}
