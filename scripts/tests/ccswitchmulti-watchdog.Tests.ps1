$corePath = Join-Path (Split-Path -Parent $PSScriptRoot) "ccswitchmulti-guardian-core.ps1"
. $corePath

function New-WatchdogTempConfig {
    param([string]$MarkerJson = "")
    $root = Join-Path $env:TEMP ("ccsm-watchdog-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path (Join-Path $root "logs") | Out-Null
    if (-not [string]::IsNullOrEmpty($MarkerJson)) {
        [System.IO.File]::WriteAllText(
            (Join-Path $root "logs\app-run-marker.json"),
            $MarkerJson,
            [System.Text.UTF8Encoding]::new($false)
        )
    }
    return $root
}

Describe "CCSwitchMulti watchdog restart policy" {
    It "never restarts a clean exit or a fresh install" {
        $root = New-WatchdogTempConfig
        (Test-CcsmGuardianUncleanExit -ConfigPath $root -GetProcessIdentity { param($ProcessId) $null }) | Should Be $false
        Remove-Item -LiteralPath $root -Recurse -Force
    }

    It "keeps a healthy run alive and blocks restart only for a dead run" {
        $root = New-WatchdogTempConfig -MarkerJson ('{ "pid": 4242 }')
        (Test-CcsmGuardianUncleanExit -ConfigPath $root -GetProcessIdentity {
            param($ProcessId)
            if ([int]$ProcessId -eq 4242) { return [pscustomobject]@{ ProcessId = 4242 } }
            return $null
        }) | Should Be $false
        (Test-CcsmGuardianUncleanExit -ConfigPath $root -GetProcessIdentity { param($ProcessId) $null }) | Should Be $true
        Remove-Item -LiteralPath $root -Recurse -Force
    }

    It "treats an unreadable marker as a crash" {
        $root = New-WatchdogTempConfig -MarkerJson ('{ "pid": 0 }')
        (Test-CcsmGuardianUncleanExit -ConfigPath $root -GetProcessIdentity { param($ProcessId) $null }) | Should Be $true
        Remove-Item -LiteralPath $root -Recurse -Force
    }

    It "rate limits restarts inside the window" {
        $now = [datetime]::UtcNow
        (Test-CcsmGuardianRestartBudget -RestartTimesUtc @() -NowUtc $now -WindowMinutes 30 -MaxRestarts 2) | Should Be $true
        (Test-CcsmGuardianRestartBudget -RestartTimesUtc @($now) -NowUtc $now -WindowMinutes 30 -MaxRestarts 2) | Should Be $true
        (Test-CcsmGuardianRestartBudget -RestartTimesUtc @($now, $now) -NowUtc $now -WindowMinutes 30 -MaxRestarts 2) | Should Be $false
        (Test-CcsmGuardianRestartBudget -RestartTimesUtc @($now.AddMinutes(-90)) -NowUtc $now -WindowMinutes 30 -MaxRestarts 1) | Should Be $true
    }
}

Describe "CCSwitchMulti watchdog loop resilience" {
    It "keeps running when a single iteration throws" {
        $events = New-Object System.Collections.Generic.List[string]
        $writeEvent = { param($Level, $Event, $Detail) $events.Add("$Level|$Event") | Out-Null }
        $ok = Invoke-CcsmGuardianSafeIteration -WriteEvent $writeEvent -Action { throw "transient wmi failure" }
        $ok | Should Be $false
        ($events -join ",") | Should Match "error\|iteration-failed"
    }

    It "reports success for a healthy iteration" {
        $events = New-Object System.Collections.Generic.List[string]
        $writeEvent = { param($Level, $Event, $Detail) $events.Add("$Level|$Event") | Out-Null }
        (Invoke-CcsmGuardianSafeIteration -WriteEvent $writeEvent -Action { $script:ran = $true }) | Should Be $true
        $events.Count | Should Be 0
    }
}
