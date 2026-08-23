[CmdletBinding()]
param(
    [string]$InstalledExecutable = "$env:LOCALAPPDATA\CCSwitchMulti\cc-switch.exe",
    [int]$Port = 15721,
    [string]$HealthUri = "http://127.0.0.1:15721/health",
    [int]$FailureThresholdSeconds = 60,
    [int]$PollSeconds = 5,
    [int]$ReadyTimeoutSeconds = 45,
    [string]$MaintenanceMarker = "$env:LOCALAPPDATA\CCSwitchMultiGuardian\maintenance.lock",
    [string]$LogPath = "$env:LOCALAPPDATA\CCSwitchMultiGuardian\guardian.jsonl",
    [string]$LockPath = "$env:LOCALAPPDATA\CCSwitchMultiGuardian\guardian.lock",
    [int]$MaxCycles = 0,
    [switch]$NoRestart,
    [switch]$PlanOnly
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot "ccswitchmulti-guardian-core.ps1")

if ($FailureThresholdSeconds -lt 1 -or $PollSeconds -lt 1 -or $ReadyTimeoutSeconds -lt 1) {
    throw "timing values must be positive"
}
$InstalledExecutable = ConvertTo-CcsmGuardianCanonicalPath -Path $InstalledExecutable

function Write-CcsmGuardianEvent {
    param([string]$Level, [string]$Event, $Detail = $null)

    $record = [ordered]@{
        timestamp = [datetime]::UtcNow.ToString("o")
        level = $Level
        event = $Event
        detail = $Detail
    }
    [System.IO.File]::AppendAllText(
        $LogPath,
        ($record | ConvertTo-Json -Compress -Depth 6) + [Environment]::NewLine,
        [System.Text.UTF8Encoding]::new($false)
    )
}

function Get-CcsmGuardianListenerOwner {
    $listener = Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue |
        Where-Object { $_.LocalAddress -in @("127.0.0.1", "0.0.0.0", "::") } |
        Select-Object -First 1
    if ($null -eq $listener) { return $null }
    return [int]$listener.OwningProcess
}

function Test-CcsmExpectedProductIdentity {
    param($Identity)

    return $null -ne $Identity -and
        (Test-CcsmGuardianSamePath -Left ([string]$Identity.Path) -Right $InstalledExecutable) -and
        [string]::Equals(
            [System.IO.Path]::GetFileName([string]$Identity.Path),
            "cc-switch.exe",
            [System.StringComparison]::OrdinalIgnoreCase
        )
}

function Test-CcsmGuardianHealth {
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri $HealthUri -TimeoutSec 3
        return [int]$response.StatusCode -ge 200 -and [int]$response.StatusCode -lt 300
    } catch {
        return $false
    }
}

function Test-CcsmUpgradeProcessActive {
    foreach ($process in @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe'" -ErrorAction SilentlyContinue)) {
        if ([string]$process.CommandLine -like "*install-ccswitchmulti-transaction.ps1*") { return $true }
    }
    foreach ($process in @(Get-CimInstance Win32_Process -Filter "Name='uninstall.exe'" -ErrorAction SilentlyContinue)) {
        if (Test-CcsmGuardianSamePath -Left ([string]$process.ExecutablePath) `
                -Right (Join-Path (Split-Path -Parent $InstalledExecutable) "uninstall.exe")) {
            return $true
        }
    }
    return @(Get-CimInstance Win32_Process -Filter "Name LIKE 'CCSwitchMulti%setup.exe'" -ErrorAction SilentlyContinue).Count -gt 0
}

function Test-CcsmGuardianMaintenance {
    $leaseActive = Test-CcsmMaintenanceLease -MarkerPath $MaintenanceMarker -NowUtc ([datetime]::UtcNow) `
        -GetProcessIdentity { param($ProcessId) Get-CcsmGuardianProcessIdentity -ProcessId $ProcessId }
    return $leaseActive -or (Test-CcsmUpgradeProcessActive)
}

function Stop-CcsmGuardianVerifiedProcess {
    param($ExpectedIdentity)

    $live = Get-CcsmGuardianProcessIdentity -ProcessId ([int]$ExpectedIdentity.ProcessId)
    if (-not (Test-CcsmExpectedProductIdentity -Identity $live) -or
        -not [string]::Equals([string]$live.StartTimeUtc, [string]$ExpectedIdentity.StartTimeUtc, [System.StringComparison]::Ordinal)) {
        throw "process identity changed before stop"
    }
    $handle = Get-Process -Id ([int]$live.ProcessId) -ErrorAction Stop
    [void]$handle.CloseMainWindow()
    if (-not $handle.WaitForExit(10000)) {
        Stop-Process -Id ([int]$live.ProcessId) -Force -ErrorAction Stop
        if (-not $handle.WaitForExit(10000)) { throw "verified CCSwitchMulti process did not exit" }
    }
}

function Wait-CcsmGuardianPortFree {
    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        if ($null -eq (Get-CcsmGuardianListenerOwner)) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

function Wait-CcsmGuardianReady {
    param([int]$ExpectedPid)

    $deadline = (Get-Date).AddSeconds($ReadyTimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if ((Get-CcsmGuardianListenerOwner) -eq $ExpectedPid -and (Test-CcsmGuardianHealth)) { return $true }
        if (-not (Get-Process -Id $ExpectedPid -ErrorAction SilentlyContinue)) { return $false }
        Start-Sleep -Seconds 1
    }
    return $false
}

$plan = [ordered]@{
    InstalledExecutable = $InstalledExecutable
    Port = $Port
    HealthUri = $HealthUri
    FailureThresholdSeconds = $FailureThresholdSeconds
    PollSeconds = $PollSeconds
    ReadyTimeoutSeconds = $ReadyTimeoutSeconds
    MaintenanceMarker = ConvertTo-CcsmGuardianCanonicalPath -Path $MaintenanceMarker
    LogPath = ConvertTo-CcsmGuardianCanonicalPath -Path $LogPath
    LockPath = ConvertTo-CcsmGuardianCanonicalPath -Path $LockPath
    NoRestart = [bool]$NoRestart
}
if ($PlanOnly) {
    $plan | ConvertTo-Json -Depth 4
    return
}

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $LockPath), (Split-Path -Parent $LogPath) | Out-Null
$lockStream = $null
try {
    $lockStream = [System.IO.File]::Open(
        $LockPath,
        [System.IO.FileMode]::OpenOrCreate,
        [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None
    )
    $state = [pscustomobject]@{ FailureSinceUtc = $null }
    $cycles = 0
    Write-CcsmGuardianEvent -Level "info" -Event "guardian-started" -Detail $plan
    while ($true) {
        $cycles++
        $isMaintenance = { Test-CcsmGuardianMaintenance }
        $writeEvent = { param($Level, $Event, $Detail) Write-CcsmGuardianEvent -Level $Level -Event $Event -Detail $Detail }
        $inspectRuntime = {
            $owner = Get-CcsmGuardianListenerOwner
            if ($null -eq $owner) { return [pscustomobject]@{ Healthy = $false; ListenerOwner = $null } }
            try {
                $identity = Get-CcsmGuardianProcessIdentity -ProcessId $owner
                $healthy = (Test-CcsmExpectedProductIdentity -Identity $identity) -and (Test-CcsmGuardianHealth)
                return [pscustomobject]@{ Healthy = $healthy; ListenerOwner = $owner }
            } catch {
                return [pscustomobject]@{ Healthy = $false; ListenerOwner = $owner }
            }
        }
        $recover = {
            if ($NoRestart) {
                Write-CcsmGuardianEvent -Level "warning" -Event "restart-suppressed" -Detail @{ Port = $Port }
                return
            }
            Invoke-CcsmGuardianRecovery -InstalledExecutable $InstalledExecutable `
                -IsMaintenance $isMaintenance `
                -InstalledExecutableExists { Test-Path -LiteralPath $InstalledExecutable -PathType Leaf } `
                -GetListenerOwner { Get-CcsmGuardianListenerOwner } `
                -GetProcessIdentity { param($ProcessId) Get-CcsmGuardianProcessIdentity -ProcessId $ProcessId } `
                -IsExpectedProductIdentity { param($Identity) Test-CcsmExpectedProductIdentity -Identity $Identity } `
                -GetExpectedProductProcesses {
                    $identities = @()
                    foreach ($process in @(Get-CimInstance Win32_Process -Filter "Name='cc-switch.exe'" -ErrorAction SilentlyContinue)) {
                        try {
                            $identity = Get-CcsmGuardianProcessIdentity -ProcessId ([int]$process.ProcessId)
                            if (Test-CcsmExpectedProductIdentity -Identity $identity) { $identities += $identity }
                        } catch { }
                    }
                    return $identities
                } `
                -StopVerifiedProductProcess { param($Identity) Stop-CcsmGuardianVerifiedProcess -ExpectedIdentity $Identity } `
                -WaitPortFree { Wait-CcsmGuardianPortFree } `
                -StartProduct { (Start-Process -FilePath $InstalledExecutable -WindowStyle Hidden -PassThru).Id } `
                -WaitReady { param($ProcessId) Wait-CcsmGuardianReady -ExpectedPid $ProcessId } `
                -WriteEvent $writeEvent
        }
        Invoke-CcsmGuardianIteration -State $state -NowUtc ([datetime]::UtcNow) `
            -FailureThresholdSeconds $FailureThresholdSeconds -IsMaintenance $isMaintenance `
            -InspectRuntime $inspectRuntime -Recover $recover -WriteEvent $writeEvent
        if ($MaxCycles -gt 0 -and $cycles -ge $MaxCycles) { break }
        Start-Sleep -Seconds $PollSeconds
    }
} catch [System.IO.IOException] {
    throw "another CCSwitchMulti guardian instance already owns $LockPath"
} finally {
    if ($null -ne $lockStream) { $lockStream.Dispose() }
}
