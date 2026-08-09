$scriptPath = Join-Path (Split-Path -Parent $PSScriptRoot) "install-ccswitchmulti-transaction.ps1"
. $scriptPath

function New-TestSpec {
    return @{
        InstallerPath            = "C:\artifacts\CCSwitchMulti-new-setup.exe"
        ExpectedInstallerHash    = ("A" * 64)
        ExpectedCurrentVersion   = "1.0.0"
        ExpectedCurrentHash      = ("B" * 64)
        ExpectedInstalledVersion = "2.0.0"
        ExpectedInstalledHash    = ("C" * 64)
        CurrentPid               = 4242
        InstalledExecutable      = "C:\Program Files\CCSwitchMulti\cc-switch.exe"
        InstallDirectory         = "C:\Program Files\CCSwitchMulti"
        UninstallExecutable      = "C:\Program Files\CCSwitchMulti\uninstall.exe"
        ConfigPaths              = @("C:\Users\test\.cc-switch")
        RegistryKey              = "HKCU:\Software\CCSwitchMulti"
        Port                     = 15721
        HealthUri                = "http://127.0.0.1:15721/health"
        TimeoutSeconds           = 30
        BackupRoot               = "D:\ccsm-transaction-backups"
    }
}

function New-FakeOperations {
    param(
        [string]$FailAt = "",
        [int]$ListenerOwner = 4242,
        [string]$NewProcessPath = "C:\Program Files\CCSwitchMulti\cc-switch.exe",
        [switch]$RollbackStartFails
    )

    $script:events = New-Object System.Collections.Generic.List[string]
    $script:checks = New-Object System.Collections.Generic.List[string]
    $script:installedNew = $false
    $script:newPid = 5000
    $script:oldPid = 6000

    return @{
        ResolvePath = {
            param($Path, $Kind)
            $script:checks.Add("resolve:$Kind") | Out-Null
            return $Path
        }
        TestPath = {
            param($Path, $Kind)
            $script:checks.Add("exists:$Kind") | Out-Null
            return $true
        }
        GetFileHash = {
            param($Path)
            $script:checks.Add("hash:$Path") | Out-Null
            if ($Path -like "*setup.exe") { return ("A" * 64) }
            if ($script:installedNew) { return ("C" * 64) }
            return ("B" * 64)
        }
        GetFileVersion = {
            param($Path)
            $script:checks.Add("version:$Path") | Out-Null
            if ($script:installedNew) { return "2.0.0" }
            return "1.0.0"
        }
        GetProcessPath = {
            param($ProcessId)
            $script:checks.Add("process-path:$ProcessId") | Out-Null
            if ($ProcessId -eq $script:newPid) { return $NewProcessPath }
            return "C:\Program Files\CCSwitchMulti\cc-switch.exe"
        }
        GetListenerOwner = {
            param($Port)
            $script:checks.Add("listener:$Port") | Out-Null
            if ($script:installedNew) { return $script:newPid }
            return $ListenerOwner
        }
        GetHealth = {
            param($Uri)
            $script:checks.Add("health:$Uri") | Out-Null
            return @{ StatusCode = 200; Healthy = $true }
        }
        WriteLog = {
            param($Context, $Level, $Event, $Detail)
            $script:events.Add("log:$Event") | Out-Null
        }
        Backup = {
            param($Context)
            $script:events.Add("backup") | Out-Null
            if ($FailAt -eq "backup") { throw "injected backup failure" }
            return @{ Path = "D:\ccsm-transaction-backups\txn-1" }
        }
        StopProcess = {
            param($Context, $ProcessId)
            $script:events.Add("stop:$ProcessId") | Out-Null
        }
        WaitPortReleased = {
            param($Context)
            $script:events.Add("wait-port-release") | Out-Null
        }
        RunUninstaller = {
            param($Context)
            $script:events.Add("uninstall") | Out-Null
        }
        RunInstaller = {
            param($Context)
            $script:events.Add("install") | Out-Null
            if ($FailAt -eq "install") { throw "injected install failure" }
            $script:installedNew = $true
        }
        StartProcess = {
            param($Context, $Mode)
            $script:events.Add("start:$Mode") | Out-Null
            if ($Mode -eq "previous") {
                if ($RollbackStartFails) { throw "injected rollback start failure" }
                $script:installedNew = $false
                return $script:oldPid
            }
            return $script:newPid
        }
        WaitReady = {
            param($Context, $ProcessId)
            $script:events.Add("wait-ready:$ProcessId") | Out-Null
            if ($FailAt -eq "ready" -and $ProcessId -eq $script:newPid) {
                throw "injected readiness failure"
            }
        }
        Restore = {
            param($Context, $Backup)
            $script:events.Add("restore") | Out-Null
            $script:installedNew = $false
        }
    }
}

Describe "CCSwitchMulti transactional reinstall orchestration" {
    It "exposes a non-mutating plan with the complete transaction and rollback boundary" {
        $plan = Get-CcsmReinstallPlan

        ($plan.Forward -join ",") | Should Be "preflight,backup,stop-verified-pid,wait-port-release,uninstall-silent,install-silent,start-hidden,wait-listener-health,verify-version-hash-path"
        ($plan.Rollback -join ",") | Should Be "verify-and-stop-new-process,restore-app-config-registry,start-previous-hidden,wait-listener-health,verify-previous-runtime"
    }

    It "runs every guard before the first mutating operation" {
        $operations = New-FakeOperations
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "Success"
        $script:events[0] | Should Be "log:preflight-ok"
        $script:events[1] | Should Be "backup"
        $script:checks | Should Contain "resolve:installer"
        $script:checks | Should Contain "resolve:installed-executable"
        $script:checks | Should Contain "resolve:install-directory"
        $script:checks | Should Contain "resolve:uninstaller"
        $script:checks | Should Contain "resolve:backup-root"
        $script:checks | Should Contain "resolve:config"
        $script:checks | Should Contain "hash:C:\artifacts\CCSwitchMulti-new-setup.exe"
        $script:checks | Should Contain "hash:C:\Program Files\CCSwitchMulti\cc-switch.exe"
        $script:checks | Should Contain "version:C:\Program Files\CCSwitchMulti\cc-switch.exe"
        $script:checks | Should Contain "process-path:4242"
        $script:checks | Should Contain "listener:15721"
    }

    It "refuses a listener owner mismatch without backup or process mutation" {
        $operations = New-FakeOperations -ListenerOwner 9999

        { Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation } |
            Should Throw "listener owner mismatch"
        ($script:events -contains "backup") | Should Be $false
        (($script:events | Where-Object { $_ -like "stop:*" }).Count) | Should Be 0
    }

    It "performs the forward transaction in exact order and verifies the new runtime" {
        $operations = New-FakeOperations
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation
        $mutations = @($script:events | Where-Object { $_ -notlike "log:*" })

        ($mutations -join ",") | Should Be "backup,stop:4242,wait-port-release,uninstall,install,start:new,wait-ready:5000"
        $script:checks | Should Contain "process-path:5000"
        $script:checks | Should Contain "health:http://127.0.0.1:15721/health"
        $result.NewPid | Should Be 5000
    }

    It "rolls back app config and registry then verifies the previous runtime after a post-stop failure" {
        $operations = New-FakeOperations -FailAt "ready"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation
        $mutations = @($script:events | Where-Object { $_ -notlike "log:*" })

        $result.Status | Should Be "RolledBack"
        ($mutations -join ",") | Should Be "backup,stop:4242,wait-port-release,uninstall,install,start:new,wait-ready:5000,stop:5000,restore,start:previous,wait-ready:6000"
        $script:checks | Should Contain "process-path:5000"
        $script:checks | Should Contain "process-path:6000"
    }

    It "never terminates a newly launched process whose executable path cannot be verified" {
        $operations = New-FakeOperations -FailAt "ready" -NewProcessPath "C:\Windows\System32\notepad.exe"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RolledBack"
        ($script:events -contains "stop:5000") | Should Be $false
        ($script:events -contains "restore") | Should Be $true
    }

    It "reports rollback failure explicitly after attempting restore and restart" {
        $operations = New-FakeOperations -FailAt "install" -RollbackStartFails
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RollbackFailed"
        ($script:events -contains "restore") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        $result.RollbackError | Should Match "rollback start failure"
    }

    It "does not attempt rollback when backup fails before the verified stop boundary" {
        $operations = New-FakeOperations -FailAt "backup"

        { Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation } |
            Should Throw "injected backup failure"
        ($script:events -contains "restore") | Should Be $false
        ($script:events -contains "start:previous") | Should Be $false
    }
}
