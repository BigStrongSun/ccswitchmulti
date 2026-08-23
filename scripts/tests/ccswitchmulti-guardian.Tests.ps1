$corePath = Join-Path (Split-Path -Parent $PSScriptRoot) "ccswitchmulti-guardian-core.ps1"
if (Test-Path -LiteralPath $corePath -PathType Leaf) {
    . $corePath
}

function Write-TestLease {
    param([string]$Path, [hashtable]$Lease)

    $directory = Split-Path -Parent $Path
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    [System.IO.File]::WriteAllText(
        $Path,
        ($Lease | ConvertTo-Json -Depth 4),
        [System.Text.UTF8Encoding]::new($false)
    )
}

function New-TestLease {
    param(
        [datetime]$ExpiresAtUtc = [datetime]"2026-08-24T05:10:00Z",
        [int]$OwnerPid = 4242,
        [string]$OwnerStartTimeUtc = "2026-08-24T05:00:00.0000000Z"
    )

    return @{
        schemaVersion = 1
        leaseId = "lease-test"
        purpose = "local-upgrade"
        ownerPid = $OwnerPid
        ownerExecutablePath = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
        ownerStartTimeUtc = $OwnerStartTimeUtc
        createdAtUtc = "2026-08-24T05:00:10.0000000Z"
        expiresAtUtc = $ExpiresAtUtc.ToUniversalTime().ToString("o")
    }
}

Describe "CCSwitchMulti guardian maintenance lease" {
    BeforeEach {
        $script:testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-guardian-test-" + [guid]::NewGuid().ToString("N"))
        $script:markerPath = Join-Path $script:testRoot "maintenance.json"
    }

    AfterEach {
        if (Test-Path -LiteralPath $script:testRoot) {
            Remove-Item -LiteralPath $script:testRoot -Recurse -Force
        }
    }

    It "accepts an unexpired lease only when PID path and start time match" {
        Write-TestLease -Path $script:markerPath -Lease (New-TestLease)
        $getIdentity = {
            param($ProcessId)
            return [pscustomobject]@{
                ProcessId = $ProcessId
                Path = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
                StartTimeUtc = "2026-08-24T05:00:00.0000000Z"
            }
        }

        Test-CcsmMaintenanceLease -MarkerPath $script:markerPath `
            -NowUtc ([datetime]"2026-08-24T05:05:00Z") -GetProcessIdentity $getIdentity | Should Be $true
    }

    It "rejects an expired lease even while the recorded PID remains alive" {
        Write-TestLease -Path $script:markerPath -Lease (New-TestLease -ExpiresAtUtc ([datetime]"2026-08-24T05:04:59Z"))
        $getIdentity = {
            param($ProcessId)
            return [pscustomobject]@{
                ProcessId = $ProcessId
                Path = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
                StartTimeUtc = "2026-08-24T05:00:00.0000000Z"
            }
        }

        Test-CcsmMaintenanceLease -MarkerPath $script:markerPath `
            -NowUtc ([datetime]"2026-08-24T05:05:00Z") -GetProcessIdentity $getIdentity | Should Be $false
    }

    It "rejects a lease when the PID has been reused by another process instance" {
        Write-TestLease -Path $script:markerPath -Lease (New-TestLease)
        $getIdentity = {
            param($ProcessId)
            return [pscustomobject]@{
                ProcessId = $ProcessId
                Path = "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
                StartTimeUtc = "2026-08-24T05:01:00.0000000Z"
            }
        }

        Test-CcsmMaintenanceLease -MarkerPath $script:markerPath `
            -NowUtc ([datetime]"2026-08-24T05:05:00Z") -GetProcessIdentity $getIdentity | Should Be $false
    }

    It "removes only the lease owned by the caller" {
        Write-TestLease -Path $script:markerPath -Lease (New-TestLease)

        Exit-CcsmMaintenanceLease -MarkerPath $script:markerPath -LeaseId "some-other-lease"
        (Test-Path -LiteralPath $script:markerPath) | Should Be $true

        Exit-CcsmMaintenanceLease -MarkerPath $script:markerPath -LeaseId "lease-test"
        (Test-Path -LiteralPath $script:markerPath) | Should Be $false
    }

    It "cleans the owned lease when the protected upgrade action fails" {
        {
            Invoke-CcsmMaintenanceLeaseScope -MarkerPath $script:markerPath -Purpose "test-upgrade" `
                -DurationSeconds 600 -Action { throw "injected upgrade failure" }
        } | Should Throw "injected upgrade failure"

        (Test-Path -LiteralPath $script:markerPath) | Should Be $false
    }

    It "atomically replaces an expired lease file instead of blocking every future upgrade" {
        Write-TestLease -Path $script:markerPath -Lease (
            New-TestLease -ExpiresAtUtc ([datetime]"2000-01-01T00:00:00Z")
        )

        $leaseId = Enter-CcsmMaintenanceLease -MarkerPath $script:markerPath `
            -Purpose "replacement-upgrade" -DurationSeconds 600
        try {
            $lease = [System.IO.File]::ReadAllText($script:markerPath, [System.Text.Encoding]::UTF8) |
                ConvertFrom-Json
            [string]$lease.leaseId | Should Be $leaseId
            [string]$lease.purpose | Should Be "replacement-upgrade"
            [int]$lease.ownerPid | Should Be $PID
        } finally {
            Exit-CcsmMaintenanceLease -MarkerPath $script:markerPath -LeaseId $leaseId
        }
    }

    It "preserves an active lease owned by the matching live process" {
        $owner = Get-CcsmGuardianProcessIdentity -ProcessId $PID
        $active = New-CcsmMaintenanceLeaseRecord -OwnerIdentity $owner -LeaseId "active-owner" `
            -NowUtc ([datetime]::UtcNow) -DurationSeconds 600 -Purpose "active-upgrade"
        Write-TestLease -Path $script:markerPath -Lease $active

        {
            Enter-CcsmMaintenanceLease -MarkerPath $script:markerPath `
                -Purpose "competing-upgrade" -DurationSeconds 600
        } | Should Throw "active CCSwitchMulti maintenance lease"

        $lease = [System.IO.File]::ReadAllText($script:markerPath, [System.Text.Encoding]::UTF8) |
            ConvertFrom-Json
        [string]$lease.leaseId | Should Be "active-owner"
    }

    It "atomically replaces malformed lease JSON" {
        New-Item -ItemType Directory -Path $script:testRoot -Force | Out-Null
        [System.IO.File]::WriteAllText(
            $script:markerPath,
            "not-json",
            [System.Text.UTF8Encoding]::new($false)
        )

        $leaseId = Enter-CcsmMaintenanceLease -MarkerPath $script:markerPath `
            -Purpose "repair-malformed" -DurationSeconds 600
        try {
            $lease = [System.IO.File]::ReadAllText($script:markerPath, [System.Text.Encoding]::UTF8) |
                ConvertFrom-Json
            [string]$lease.leaseId | Should Be $leaseId
        } finally {
            Exit-CcsmMaintenanceLease -MarkerPath $script:markerPath -LeaseId $leaseId
        }
    }
}

Describe "CCSwitchMulti owned child process wait" {
    It "returns when the transaction owner exits without waiting for its long-running descendant" {
        $testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-process-wait-" + [guid]::NewGuid().ToString("N"))
        $spawnScript = Join-Path $testRoot "spawn-descendant.ps1"
        $childPidPath = Join-Path $testRoot "child.pid"
        $parent = $null
        $childPid = 0
        New-Item -ItemType Directory -Path $testRoot -Force | Out-Null
        [System.IO.File]::WriteAllText(
            $spawnScript,
            @'
param([string]$ChildPidPath)
$powershell = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
$child = Start-Process -FilePath $powershell -WindowStyle Hidden -PassThru `
    -ArgumentList @("-NoProfile", "-Command", "Start-Sleep -Seconds 30")
[System.IO.File]::WriteAllText(
    $ChildPidPath,
    [string]$child.Id,
    [System.Text.UTF8Encoding]::new($false)
)
'@,
            [System.Text.UTF8Encoding]::new($false)
        )
        try {
            $powershell = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
            $parent = Start-Process -FilePath $powershell -WindowStyle Hidden -PassThru `
                -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $spawnScript, $childPidPath)
            $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

            $exitCode = Wait-CcsmOwnedProcessExit -Process $parent

            $stopwatch.Stop()
            $exitCode | Should Be 0
            $stopwatch.Elapsed.TotalSeconds | Should BeLessThan 10
            (Test-Path -LiteralPath $childPidPath -PathType Leaf) | Should Be $true
            $childPid = [int][System.IO.File]::ReadAllText($childPidPath, [System.Text.Encoding]::UTF8)
            (Get-Process -Id $childPid -ErrorAction SilentlyContinue) | Should Not BeNullOrEmpty
        } finally {
            if ($childPid -gt 0) { Stop-Process -Id $childPid -Force -ErrorAction SilentlyContinue }
            if ($null -ne $parent -and -not $parent.HasExited) { Stop-Process -Id $parent.Id -Force -ErrorAction SilentlyContinue }
            if (Test-Path -LiteralPath $testRoot) { Remove-Item -LiteralPath $testRoot -Recurse -Force }
        }
    }
}

Describe "CCSwitchMulti guardian decision loop" {
    BeforeEach {
        $script:events = New-Object System.Collections.Generic.List[string]
        $script:recoveries = 0
        $script:writeEvent = {
            param($Level, $Event, $Detail)
            $script:events.Add($Event) | Out-Null
        }
        $script:recover = { $script:recoveries++ }
    }

    It "does nothing while the expected listener is healthy" {
        $state = [pscustomobject]@{ FailureSinceUtc = $null }
        $inspect = { [pscustomobject]@{ Healthy = $true; ListenerOwner = 4242 } }

        Invoke-CcsmGuardianIteration -State $state -NowUtc ([datetime]"2026-08-24T05:00:00Z") `
            -FailureThresholdSeconds 60 -IsMaintenance { $false } -InspectRuntime $inspect `
            -Recover $script:recover -WriteEvent $script:writeEvent

        $script:recoveries | Should Be 0
        $state.FailureSinceUtc | Should Be $null
    }

    It "suppresses recovery and clears the failure timer during active maintenance" {
        $state = [pscustomobject]@{ FailureSinceUtc = [datetime]"2026-08-24T04:58:00Z" }
        $inspect = { throw "runtime inspection must not run during maintenance" }

        Invoke-CcsmGuardianIteration -State $state -NowUtc ([datetime]"2026-08-24T05:00:00Z") `
            -FailureThresholdSeconds 60 -IsMaintenance { $true } -InspectRuntime $inspect `
            -Recover $script:recover -WriteEvent $script:writeEvent

        $script:recoveries | Should Be 0
        $state.FailureSinceUtc | Should Be $null
    }

    It "waits for a continuous sixty-second failure before recovery" {
        $state = [pscustomobject]@{ FailureSinceUtc = $null }
        $inspect = { [pscustomobject]@{ Healthy = $false; ListenerOwner = $null } }

        Invoke-CcsmGuardianIteration -State $state -NowUtc ([datetime]"2026-08-24T05:00:00Z") `
            -FailureThresholdSeconds 60 -IsMaintenance { $false } -InspectRuntime $inspect `
            -Recover $script:recover -WriteEvent $script:writeEvent
        Invoke-CcsmGuardianIteration -State $state -NowUtc ([datetime]"2026-08-24T05:00:59Z") `
            -FailureThresholdSeconds 60 -IsMaintenance { $false } -InspectRuntime $inspect `
            -Recover $script:recover -WriteEvent $script:writeEvent
        $script:recoveries | Should Be 0

        Invoke-CcsmGuardianIteration -State $state -NowUtc ([datetime]"2026-08-24T05:01:00Z") `
            -FailureThresholdSeconds 60 -IsMaintenance { $false } -InspectRuntime $inspect `
            -Recover $script:recover -WriteEvent $script:writeEvent
        $script:recoveries | Should Be 1
    }
}

Describe "CCSwitchMulti guarded recovery" {
    BeforeEach {
        $script:actions = New-Object System.Collections.Generic.List[string]
        $script:writeEvent = {
            param($Level, $Event, $Detail)
            $script:actions.Add("event:$Event") | Out-Null
        }
    }

    It "refuses to stop a foreign listener" {
        Invoke-CcsmGuardianRecovery -InstalledExecutable "C:\Apps\CCSwitchMulti\cc-switch.exe" `
            -IsMaintenance { $false } -InstalledExecutableExists { $true } -GetListenerOwner { 9001 } `
            -GetProcessIdentity { param($ProcessId) [pscustomobject]@{ ProcessId = $ProcessId; Path = "C:\Windows\notepad.exe"; StartTimeUtc = "2026-08-24T05:00:00Z" } } `
            -IsExpectedProductIdentity { param($Identity) $false } `
            -GetExpectedProductProcesses { @() } `
            -StopVerifiedProductProcess { param($Identity) $script:actions.Add("stop") | Out-Null } `
            -WaitPortFree { $script:actions.Add("wait-port") | Out-Null; $true } `
            -StartProduct { $script:actions.Add("start") | Out-Null; 5000 } `
            -WaitReady { param($ProcessId) $true } -WriteEvent $script:writeEvent

        (@($script:actions | Where-Object { $_ -eq "stop" }).Count) | Should Be 0
        (@($script:actions | Where-Object { $_ -eq "start" }).Count) | Should Be 0
        ($script:actions -contains "event:foreign-listener-blocked-recovery") | Should Be $true
    }

    It "stops verified stale processes and waits for port release before start" {
        $identity = [pscustomobject]@{ ProcessId = 4242; Path = "C:\Apps\CCSwitchMulti\cc-switch.exe"; StartTimeUtc = "2026-08-24T05:00:00Z" }
        Invoke-CcsmGuardianRecovery -InstalledExecutable "C:\Apps\CCSwitchMulti\cc-switch.exe" `
            -IsMaintenance { $false } -InstalledExecutableExists { $true } -GetListenerOwner { $null } `
            -GetProcessIdentity { param($ProcessId) $identity } `
            -IsExpectedProductIdentity { param($Identity) $true } `
            -GetExpectedProductProcesses { @($identity) } `
            -StopVerifiedProductProcess { param($Identity) $script:actions.Add("stop:$($Identity.ProcessId)") | Out-Null } `
            -WaitPortFree { $script:actions.Add("wait-port") | Out-Null; $true } `
            -StartProduct { $script:actions.Add("start") | Out-Null; 5000 } `
            -WaitReady { param($ProcessId) $script:actions.Add("ready:$ProcessId") | Out-Null; $true } `
            -WriteEvent $script:writeEvent

        $script:actions.IndexOf("stop:4242") | Should BeLessThan $script:actions.IndexOf("wait-port")
        $script:actions.IndexOf("wait-port") | Should BeLessThan $script:actions.IndexOf("start")
        $script:actions.IndexOf("start") | Should BeLessThan $script:actions.IndexOf("ready:5000")
    }
}
