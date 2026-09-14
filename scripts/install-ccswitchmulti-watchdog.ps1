[CmdletBinding()]
param(
    [string]$GuardianRoot = "$env:LOCALAPPDATA\CCSwitchMultiGuardian",
    [string]$TaskName = "CCSwitchMulti-Watchdog",
    [string]$InstalledExecutable = "$env:LOCALAPPDATA\CCSwitchMulti\cc-switch.exe",
    [int]$Port = 15721,
    [int]$PollSeconds = 5,
    [int]$FailureThresholdSeconds = 60,
    [string]$ConfigPath = "$env:USERPROFILE\.cc-switch",
    [switch]$Uninstall,
    [switch]$NoStart
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$watcherSource = Join-Path $PSScriptRoot "watch-ccswitchmulti.ps1"
$coreSource = Join-Path $PSScriptRoot "ccswitchmulti-guardian-core.ps1"
$watcherTarget = Join-Path $GuardianRoot "watch-ccswitchmulti.ps1"
$coreTarget = Join-Path $GuardianRoot "ccswitchmulti-guardian-core.ps1"
$logPath = Join-Path $GuardianRoot "guardian.jsonl"
$lockPath = Join-Path $GuardianRoot "guardian.lock"

function Get-CcsmWatchdogProcesses {
    return @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe'" -ErrorAction SilentlyContinue |
        Where-Object { [string]$_.CommandLine -like "*$watcherTarget*" })
}

if ($Uninstall) {
    $task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($null -ne $task) {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction Stop
    }
    $stopped = @()
    foreach ($process in (Get-CcsmWatchdogProcesses)) {
        Stop-Process -Id ([int]$process.ProcessId) -Force -ErrorAction SilentlyContinue
        $stopped += [int]$process.ProcessId
    }
    [pscustomobject]@{ TaskRemoved = $null -ne $task; StoppedProcesses = $stopped } | ConvertTo-Json -Depth 4
    return
}

foreach ($required in @($watcherSource, $coreSource, $InstalledExecutable)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "required file missing: $required" }
}

New-Item -ItemType Directory -Force -Path $GuardianRoot | Out-Null
foreach ($pair in @(@{ Source = $watcherSource; Target = $watcherTarget }, @{ Source = $coreSource; Target = $coreTarget })) {
    $text = [System.IO.File]::ReadAllText($pair.Source, [System.Text.UTF8Encoding]::new($false))
    [System.IO.File]::WriteAllText($pair.Target, $text, [System.Text.UTF8Encoding]::new($false))
}

$arguments = @(
    "-NoProfile"
    "-WindowStyle"
    "Hidden"
    "-ExecutionPolicy"
    "Bypass"
    "-File"
    "`"$watcherTarget`""
    "-InstalledExecutable"
    "`"$InstalledExecutable`""
    "-Port"
    "$Port"
    "-PollSeconds"
    "$PollSeconds"
    "-FailureThresholdSeconds"
    "$FailureThresholdSeconds"
    "-ConfigPath"
    "`"$ConfigPath`""
) -join " "
$action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument $arguments
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -MultipleInstances IgnoreNew -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1)
$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Settings $settings -Principal $principal -Force | Out-Null

$startedAt = [datetime]::UtcNow
if (-not $NoStart) {
    Start-ScheduledTask -TaskName $TaskName
    $deadline = (Get-Date).AddSeconds(45)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Seconds 2
        $processes = @(Get-CcsmWatchdogProcesses)
        if ($processes.Count -gt 0 -and (Test-Path -LiteralPath $lockPath -PathType Leaf)) { break }
    }
}

$events = @()
if (Test-Path -LiteralPath $logPath -PathType Leaf) {
    $events = @([System.IO.File]::ReadAllLines($logPath, [System.Text.Encoding]::UTF8) | Select-Object -Last 6)
}
$taskState = (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue).State
$result = [ordered]@{
    TaskName = $TaskName
    TaskState = [string]$taskState
    GuardianRoot = $GuardianRoot
    WatcherProcesses = @((Get-CcsmWatchdogProcesses) | ForEach-Object { [int]$_.ProcessId })
    LockExists = (Test-Path -LiteralPath $lockPath -PathType Leaf)
    LogPath = $logPath
    RecentEvents = $events
    RequestedAtUtc = $startedAt.ToString("o")
}
$result | ConvertTo-Json -Depth 6
