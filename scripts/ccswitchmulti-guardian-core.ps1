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

function Test-CcsmMaintenanceLeaseRecord {
    param(
        [Parameter(Mandatory = $true)]$Lease,
        [Parameter(Mandatory = $true)][datetime]$NowUtc,
        [Parameter(Mandatory = $true)][scriptblock]$GetProcessIdentity
    )

    try {
        if ([int]$Lease.schemaVersion -ne 1 -or
            [string]::IsNullOrWhiteSpace([string]$Lease.leaseId) -or
            [int]$Lease.ownerPid -lt 1 -or
            [string]::IsNullOrWhiteSpace([string]$Lease.ownerExecutablePath) -or
            [string]::IsNullOrWhiteSpace([string]$Lease.ownerStartTimeUtc) -or
            [string]::IsNullOrWhiteSpace([string]$Lease.expiresAtUtc)) {
            return $false
        }
        $expiresAt = ConvertTo-CcsmGuardianUtc -Value ([string]$Lease.expiresAtUtc)
        if ($expiresAt -le [datetimeoffset]$NowUtc.ToUniversalTime()) { return $false }

        $owner = & $GetProcessIdentity ([int]$Lease.ownerPid)
        if ($null -eq $owner -or [int]$owner.ProcessId -ne [int]$Lease.ownerPid) { return $false }
        if (-not (Test-CcsmGuardianSamePath -Left ([string]$owner.Path) -Right ([string]$Lease.ownerExecutablePath))) {
            return $false
        }
        $expectedStart = ConvertTo-CcsmGuardianUtc -Value ([string]$Lease.ownerStartTimeUtc)
        $actualStart = ConvertTo-CcsmGuardianUtc -Value ([string]$owner.StartTimeUtc)
        return $expectedStart.UtcTicks -eq $actualStart.UtcTicks
    } catch {
        return $false
    }
}

function Enter-CcsmMaintenanceLease {
    param(
        [Parameter(Mandatory = $true)][string]$MarkerPath,
        [Parameter(Mandatory = $true)][string]$Purpose,
        [Parameter(Mandatory = $true)][int]$DurationSeconds
    )

    $nowUtc = [datetime]::UtcNow
    $owner = Get-CcsmGuardianProcessIdentity -ProcessId $PID
    $leaseId = [guid]::NewGuid().ToString("N")
    $record = New-CcsmMaintenanceLeaseRecord -OwnerIdentity $owner -LeaseId $leaseId `
        -NowUtc $nowUtc -DurationSeconds $DurationSeconds -Purpose $Purpose
    $directory = Split-Path -Parent $MarkerPath
    New-Item -ItemType Directory -Path $directory -Force | Out-Null

    $stream = [System.IO.File]::Open(
        $MarkerPath,
        [System.IO.FileMode]::OpenOrCreate,
        [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None
    )
    try {
        $existingLease = $null
        if ($stream.Length -gt 0 -and $stream.Length -le 65536) {
            try {
                $stream.Position = 0
                $buffer = New-Object byte[] ([int]$stream.Length)
                $offset = 0
                while ($offset -lt $buffer.Length) {
                    $count = $stream.Read($buffer, $offset, $buffer.Length - $offset)
                    if ($count -le 0) { break }
                    $offset += $count
                }
                if ($offset -eq $buffer.Length) {
                    $existingLease = [System.Text.Encoding]::UTF8.GetString($buffer) | ConvertFrom-Json
                }
            } catch {
                $existingLease = $null
            }
        }
        if ($null -ne $existingLease -and (Test-CcsmMaintenanceLeaseRecord `
                    -Lease $existingLease -NowUtc $nowUtc `
                    -GetProcessIdentity { param($ProcessId) Get-CcsmGuardianProcessIdentity -ProcessId $ProcessId })) {
            throw "an active CCSwitchMulti maintenance lease already owns $MarkerPath"
        }

        $stream.SetLength(0)
        $stream.Position = 0
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
        return Test-CcsmMaintenanceLeaseRecord -Lease $lease -NowUtc $NowUtc `
            -GetProcessIdentity $GetProcessIdentity
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

function Wait-CcsmOwnedProcessExit {
    param([Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process)

    $Process.WaitForExit()
    return [int]$Process.ExitCode
}

function Get-CcsmGuardianFileSha256 {
    param([Parameter(Mandatory = $true)][string]$LiteralPath)

    $stream = [System.IO.File]::Open(
        $LiteralPath,
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::Read
    )
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [System.BitConverter]::ToString($sha.ComputeHash($stream)).Replace("-", "")
    } finally {
        $sha.Dispose()
        $stream.Dispose()
    }
}

function Get-CcsmTauriNsisPayloadHash {
    param([Parameter(Mandatory = $true)][string]$ExecutablePath)

    if (-not (Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
        throw "Tauri executable missing: $ExecutablePath"
    }
    $unknownMarker = "__TAURI_BUNDLE_TYPE_VAR_UNK"
    $nsisMarker = "__TAURI_BUNDLE_TYPE_VAR_NSS"
    if ($unknownMarker.Length -ne $nsisMarker.Length) {
        throw "Tauri bundle markers must have equal length"
    }

    $bytes = [System.IO.File]::ReadAllBytes($ExecutablePath)
    $ascii = [System.Text.Encoding]::ASCII.GetString($bytes)
    $markerIndex = $ascii.IndexOf($unknownMarker, [System.StringComparison]::Ordinal)
    if ($markerIndex -lt 0) { throw "Tauri unknown bundle marker not found" }
    if ($ascii.IndexOf($unknownMarker, $markerIndex + 1, [System.StringComparison]::Ordinal) -ge 0) {
        throw "multiple Tauri unknown bundle markers found"
    }

    $replacement = [System.Text.Encoding]::ASCII.GetBytes($nsisMarker)
    [System.Array]::Copy($replacement, 0, $bytes, $markerIndex, $replacement.Length)
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [System.BitConverter]::ToString($sha.ComputeHash($bytes)).Replace("-", "")
    } finally {
        $sha.Dispose()
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
