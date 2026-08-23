$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function ConvertTo-CcsmGuardianCanonicalPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    return [System.IO.Path]::GetFullPath($Path).TrimEnd([char[]]@([char]92, [char]47))
}

function Test-CcsmGuardianSamePath {
    param([string]$Left, [string]$Right)

    if ([string]::IsNullOrWhiteSpace($Left) -or [string]::IsNullOrWhiteSpace($Right)) {
        return $false
    }
    return [string]::Equals(
        (ConvertTo-CcsmGuardianCanonicalPath -Path $Left),
        (ConvertTo-CcsmGuardianCanonicalPath -Path $Right),
        [System.StringComparison]::OrdinalIgnoreCase
    )
}

function ConvertTo-CcsmGuardianUtc {
    param([Parameter(Mandatory = $true)][string]$Value)

    $parsed = [datetimeoffset]::MinValue
    $styles = [System.Globalization.DateTimeStyles]::AssumeUniversal -bor
        [System.Globalization.DateTimeStyles]::AdjustToUniversal
    if (-not [datetimeoffset]::TryParse(
            $Value,
            [System.Globalization.CultureInfo]::InvariantCulture,
            $styles,
            [ref]$parsed
        )) {
        throw "invalid UTC timestamp"
    }
    return $parsed.ToUniversalTime()
}

function Get-CcsmGuardianProcessIdentity {
    param([Parameter(Mandatory = $true)][int]$ProcessId)

    $process = Get-CimInstance Win32_Process -Filter "ProcessId=$ProcessId" -ErrorAction Stop
    $handle = Get-Process -Id $ProcessId -ErrorAction Stop
    return [pscustomobject]@{
        ProcessId = $ProcessId
        Path = [string]$process.ExecutablePath
        CommandLine = [string]$process.CommandLine
        StartTimeUtc = $handle.StartTime.ToUniversalTime().ToString("o")
    }
}

function New-CcsmMaintenanceLeaseRecord {
    param(
        [Parameter(Mandatory = $true)]$OwnerIdentity,
        [Parameter(Mandatory = $true)][string]$LeaseId,
        [Parameter(Mandatory = $true)][datetime]$NowUtc,
        [Parameter(Mandatory = $true)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)][string]$Purpose
    )

    if ($DurationSeconds -lt 1) { throw "maintenance lease duration must be positive" }
    if ([string]::IsNullOrWhiteSpace($LeaseId)) { throw "maintenance lease ID must not be empty" }
    if ([string]::IsNullOrWhiteSpace([string]$OwnerIdentity.Path)) { throw "maintenance lease owner path must not be empty" }

    $created = $NowUtc.ToUniversalTime()
    return [ordered]@{
        schemaVersion = 1
        leaseId = $LeaseId
        purpose = $Purpose
        ownerPid = [int]$OwnerIdentity.ProcessId
        ownerExecutablePath = ConvertTo-CcsmGuardianCanonicalPath -Path ([string]$OwnerIdentity.Path)
        ownerStartTimeUtc = [string]$OwnerIdentity.StartTimeUtc
        createdAtUtc = $created.ToString("o")
        expiresAtUtc = $created.AddSeconds($DurationSeconds).ToString("o")
    }
}

function Enter-CcsmMaintenanceLease {
    param(
        [Parameter(Mandatory = $true)][string]$MarkerPath,
        [Parameter(Mandatory = $true)][string]$Purpose,
        [Parameter(Mandatory = $true)][int]$DurationSeconds
    )

    $owner = Get-CcsmGuardianProcessIdentity -ProcessId $PID
    $leaseId = [guid]::NewGuid().ToString("N")
    $record = New-CcsmMaintenanceLeaseRecord -OwnerIdentity $owner -LeaseId $leaseId `
        -NowUtc ([datetime]::UtcNow) -DurationSeconds $DurationSeconds -Purpose $Purpose
    $directory = Split-Path -Parent $MarkerPath
    New-Item -ItemType Directory -Path $directory -Force | Out-Null

    $stream = [System.IO.File]::Open(
        $MarkerPath,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    try {
        $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes(($record | ConvertTo-Json -Depth 4))
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
    return $leaseId
}

function Test-CcsmMaintenanceLease {
    param(
        [Parameter(Mandatory = $true)][string]$MarkerPath,
        [Parameter(Mandatory = $true)][datetime]$NowUtc,
        [Parameter(Mandatory = $true)][scriptblock]$GetProcessIdentity
    )

    if (-not (Test-Path -LiteralPath $MarkerPath -PathType Leaf)) { return $false }
    try {
        $lease = [System.IO.File]::ReadAllText($MarkerPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json
        if ([int]$lease.schemaVersion -ne 1 -or
            [string]::IsNullOrWhiteSpace([string]$lease.leaseId) -or
            [int]$lease.ownerPid -lt 1 -or
            [string]::IsNullOrWhiteSpace([string]$lease.ownerExecutablePath) -or
            [string]::IsNullOrWhiteSpace([string]$lease.ownerStartTimeUtc) -or
            [string]::IsNullOrWhiteSpace([string]$lease.expiresAtUtc)) {
            return $false
        }
        $expiresAt = ConvertTo-CcsmGuardianUtc -Value ([string]$lease.expiresAtUtc)
        if ($expiresAt -le [datetimeoffset]$NowUtc.ToUniversalTime()) { return $false }

        $owner = & $GetProcessIdentity ([int]$lease.ownerPid)
        if ($null -eq $owner -or [int]$owner.ProcessId -ne [int]$lease.ownerPid) { return $false }
        if (-not (Test-CcsmGuardianSamePath -Left ([string]$owner.Path) -Right ([string]$lease.ownerExecutablePath))) {
            return $false
        }
        $expectedStart = ConvertTo-CcsmGuardianUtc -Value ([string]$lease.ownerStartTimeUtc)
        $actualStart = ConvertTo-CcsmGuardianUtc -Value ([string]$owner.StartTimeUtc)
        return $expectedStart.UtcTicks -eq $actualStart.UtcTicks
    } catch {
        return $false
    }
}

function Exit-CcsmMaintenanceLease {
    param(
        [Parameter(Mandatory = $true)][string]$MarkerPath,
        [Parameter(Mandatory = $true)][string]$LeaseId
    )

    if (-not (Test-Path -LiteralPath $MarkerPath -PathType Leaf)) { return }
    try {
        $lease = [System.IO.File]::ReadAllText($MarkerPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json
        if ([string]::Equals([string]$lease.leaseId, $LeaseId, [System.StringComparison]::Ordinal)) {
            Remove-Item -LiteralPath $MarkerPath -Force -ErrorAction Stop
        }
    } catch {
        return
    }
}

function Invoke-CcsmMaintenanceLeaseScope {
    param(
        [Parameter(Mandatory = $true)][string]$MarkerPath,
        [Parameter(Mandatory = $true)][string]$Purpose,
        [Parameter(Mandatory = $true)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )

    $leaseId = Enter-CcsmMaintenanceLease -MarkerPath $MarkerPath -Purpose $Purpose `
        -DurationSeconds $DurationSeconds
    try {
        return & $Action $leaseId
    } finally {
        Exit-CcsmMaintenanceLease -MarkerPath $MarkerPath -LeaseId $leaseId
    }
}

function Invoke-CcsmGuardianIteration {
    param(
        [Parameter(Mandatory = $true)]$State,
        [Parameter(Mandatory = $true)][datetime]$NowUtc,
        [Parameter(Mandatory = $true)][int]$FailureThresholdSeconds,
        [Parameter(Mandatory = $true)][scriptblock]$IsMaintenance,
        [Parameter(Mandatory = $true)][scriptblock]$InspectRuntime,
        [Parameter(Mandatory = $true)][scriptblock]$Recover,
        [Parameter(Mandatory = $true)][scriptblock]$WriteEvent
    )

    if (& $IsMaintenance) {
        $State.FailureSinceUtc = $null
        return
    }

    $runtime = & $InspectRuntime
    if ([bool]$runtime.Healthy) {
        $State.FailureSinceUtc = $null
        return
    }

    $now = $NowUtc.ToUniversalTime()
    if ($null -eq $State.FailureSinceUtc) {
        $State.FailureSinceUtc = $now
        & $WriteEvent "warning" "health-loss-detected" @{ Owner = $runtime.ListenerOwner }
        return
    }

    $elapsed = ($now - ([datetime]$State.FailureSinceUtc).ToUniversalTime()).TotalSeconds
    if ($elapsed -lt $FailureThresholdSeconds) { return }

    & $WriteEvent "warning" "health-loss-threshold-reached" @{ ElapsedSeconds = [int]$elapsed }
    & $Recover
    $State.FailureSinceUtc = $now
}

function Invoke-CcsmGuardianRecovery {
    param(
        [Parameter(Mandatory = $true)][string]$InstalledExecutable,
        [Parameter(Mandatory = $true)][scriptblock]$IsMaintenance,
        [Parameter(Mandatory = $true)][scriptblock]$InstalledExecutableExists,
        [Parameter(Mandatory = $true)][scriptblock]$GetListenerOwner,
        [Parameter(Mandatory = $true)][scriptblock]$GetProcessIdentity,
        [Parameter(Mandatory = $true)][scriptblock]$IsExpectedProductIdentity,
        [Parameter(Mandatory = $true)][scriptblock]$GetExpectedProductProcesses,
        [Parameter(Mandatory = $true)][scriptblock]$StopVerifiedProductProcess,
        [Parameter(Mandatory = $true)][scriptblock]$WaitPortFree,
        [Parameter(Mandatory = $true)][scriptblock]$StartProduct,
        [Parameter(Mandatory = $true)][scriptblock]$WaitReady,
        [Parameter(Mandatory = $true)][scriptblock]$WriteEvent
    )

    if (& $IsMaintenance) {
        & $WriteEvent "info" "recovery-deferred-maintenance" $null
        return
    }
    if (-not (& $InstalledExecutableExists)) {
        & $WriteEvent "error" "installed-executable-missing" @{ Path = $InstalledExecutable }
        return
    }

    $stopped = New-Object 'System.Collections.Generic.HashSet[int]'
    $ownerPid = & $GetListenerOwner
    if ($null -ne $ownerPid) {
        try {
            $ownerIdentity = & $GetProcessIdentity ([int]$ownerPid)
        } catch {
            & $WriteEvent "warning" "listener-owner-transient" @{ ProcessId = $ownerPid }
            return
        }
        if (-not (& $IsExpectedProductIdentity $ownerIdentity)) {
            & $WriteEvent "error" "foreign-listener-blocked-recovery" @{
                ProcessId = $ownerPid
                Path = [string]$ownerIdentity.Path
            }
            return
        }
        & $WriteEvent "warning" "stopping-unhealthy-product" @{ ProcessId = $ownerPid }
        & $StopVerifiedProductProcess $ownerIdentity
        [void]$stopped.Add([int]$ownerPid)
    }

    foreach ($identity in @(& $GetExpectedProductProcesses)) {
        if ($null -eq $identity -or $stopped.Contains([int]$identity.ProcessId)) { continue }
        if (-not (& $IsExpectedProductIdentity $identity)) { continue }
        & $WriteEvent "warning" "stopping-stale-product" @{ ProcessId = [int]$identity.ProcessId }
        & $StopVerifiedProductProcess $identity
        [void]$stopped.Add([int]$identity.ProcessId)
    }

    if (-not (& $WaitPortFree)) {
        & $WriteEvent "error" "port-did-not-release" @{ Owner = (& $GetListenerOwner) }
        return
    }
    if (& $IsMaintenance) {
        & $WriteEvent "info" "recovery-deferred-maintenance" $null
        return
    }

    $startedPid = [int](& $StartProduct)
    & $WriteEvent "warning" "product-started" @{ ProcessId = $startedPid; Path = $InstalledExecutable }
    if (& $WaitReady $startedPid) {
        & $WriteEvent "info" "recovery-ready" @{ ProcessId = $startedPid }
    } else {
        & $WriteEvent "error" "recovery-not-ready" @{ ProcessId = $startedPid }
    }
}
