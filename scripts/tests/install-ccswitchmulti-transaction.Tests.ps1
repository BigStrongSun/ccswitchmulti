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
        RegistryKey              = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti"
        Port                     = 15721
        HealthUri                = "http://127.0.0.1:15721/health"
        TimeoutSeconds           = 30
        BackupRoot               = "D:\ccsm-transaction-backups"
    }
}

function New-FakeOperations {
    param(
        [string[]]$FailAt = @(),
        [int]$ListenerOwner = 4242,
        [string]$NewProcessPath = "C:\Program Files\CCSwitchMulti\cc-switch.exe",
        [switch]$RollbackStartFails,
        [string[]]$FailLogEvent = @(),
        [switch]$ChangeIdentityAfterBackup,
        [switch]$UnhealthyPreflight,
        [string]$BackupTamper = "",
        [switch]$TraceDestructiveRestore
    )

    $script:events = New-Object System.Collections.Generic.List[string]
    $script:checks = New-Object System.Collections.Generic.List[string]
    $script:installedNew = $false
    $script:newPid = 5000
    $script:oldPid = 6000
    $script:runtimePid = $ListenerOwner
    $script:failAt = $FailAt
    $script:newProcessPath = $NewProcessPath
    $script:rollbackStartFails = [bool]$RollbackStartFails
    $script:failLogEvent = $FailLogEvent
    $script:changeIdentityAfterBackup = [bool]$ChangeIdentityAfterBackup
    $script:identityChanged = $false
    $script:unhealthyPreflight = [bool]$UnhealthyPreflight
    $script:backupTamper = $BackupTamper
    $script:traceDestructiveRestore = [bool]$TraceDestructiveRestore
    $script:currentProcessPath = "C:\Program Files\CCSwitchMulti\cc-switch.exe"
    $script:currentProcessStartTime = "2026-08-09T08:00:00.0000000Z"

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
            if ($ProcessId -eq $script:newPid) {
                if ($script:newProcessPath -eq "__missing__") { throw "injected process not found" }
                return $script:newProcessPath
            }
            if ($ProcessId -eq 4242) {
                return $script:currentProcessPath
            }
            return "C:\Program Files\CCSwitchMulti\cc-switch.exe"
        }
        GetProcessIdentity = {
            param($ProcessId)
            $script:checks.Add("process-identity:$ProcessId") | Out-Null
            if ($ProcessId -eq $script:newPid) {
                if ($script:newProcessPath -eq "__missing__") { throw "injected process not found" }
                return @{
                    ProcessId = $script:newPid
                    Path = $script:newProcessPath
                    StartTime = "2026-08-09T08:10:00.0000000Z"
                }
            }
            if ($ProcessId -eq 4242) {
                return @{
                    ProcessId = 4242
                    Path = $script:currentProcessPath
                    StartTime = $script:currentProcessStartTime
                }
            }
            return @{
                ProcessId = $ProcessId
                Path = "C:\Program Files\CCSwitchMulti\cc-switch.exe"
                StartTime = "2026-08-09T08:10:00.0000000Z"
            }
        }
        GetListenerOwner = {
            param($Port)
            $script:checks.Add("listener:$Port") | Out-Null
            return $script:runtimePid
        }
        GetHealth = {
            param($Uri)
            $script:checks.Add("health:$Uri") | Out-Null
            if ($script:unhealthyPreflight) {
                return @{ StatusCode = 503; Healthy = $false }
            }
            return @{ StatusCode = 200; Healthy = $true }
        }
        WriteLog = {
            param($Context, $Level, $Event, $Detail)
            $script:events.Add("log:$Event") | Out-Null
            if ($script:failLogEvent -contains $Event) {
                throw "injected log failure: $Event"
            }
        }
        Backup = {
            param($Context)
            $script:events.Add("backup") | Out-Null
            if ($script:failAt -contains "backup") { throw "injected backup failure" }
            if ($script:changeIdentityAfterBackup) {
                $script:identityChanged = $true
                $script:currentProcessPath = "C:\Windows\System32\notepad.exe"
                $script:currentProcessStartTime = "2026-08-09T08:05:00.0000000Z"
            }
            return @{
                Path = "D:\ccsm-transaction-backups\txn-1"
                Manifest = @{ Tamper = $script:backupTamper }
            }
        }
        StopProcess = {
            param($Context, $ProcessId)
            if ($ProcessId -eq 4242 -and $script:identityChanged) {
                $script:events.Add("unsafe-stop:$ProcessId") | Out-Null
                throw "attempted to stop a changed process identity"
            }
            $script:events.Add("stop:$ProcessId") | Out-Null
        }
        StopVerifiedProcess = {
            param($Context, $ExpectedIdentity)
            $processId = [int]$ExpectedIdentity.ProcessId
            if ($processId -eq 4242 -and $script:identityChanged) {
                $script:events.Add("unsafe-stop:$processId") | Out-Null
                throw "attempted to stop a changed process identity"
            }
            $script:events.Add("stop:$processId") | Out-Null
        }
        WaitPortReleased = {
            param($Context)
            $script:events.Add("wait-port-release") | Out-Null
        }
        SnapshotConfig = {
            param($Context, $Backup)
            $script:events.Add("snapshot-config") | Out-Null
            return @{
                Files = @(
                    @{ RelativePath = "cc-switch.db"; Hash = ("D" * 64) },
                    @{ RelativePath = "cc-switch.db-wal"; Hash = ("E" * 64) },
                    @{ RelativePath = "cc-switch.db-shm"; Hash = ("F" * 64) }
                )
                Integrity = "ok"
            }
        }
        VerifyConfigSnapshot = {
            param($Context, $Backup)
            $script:events.Add("verify-config-snapshot") | Out-Null
        }
        RunUninstaller = {
            param($Context)
            $script:events.Add("uninstall") | Out-Null
        }
        RunInstaller = {
            param($Context)
            $script:events.Add("install") | Out-Null
            if ($script:failAt -contains "install") { throw "injected install failure" }
            $script:installedNew = $true
        }
        StartProcess = {
            param($Context, $Mode)
            $script:events.Add("start:$Mode") | Out-Null
            if ($Mode -eq "previous") {
                if ($script:rollbackStartFails) { throw "injected rollback start failure" }
                $script:installedNew = $false
                $script:runtimePid = $script:oldPid
                return $script:oldPid
            }
            $script:runtimePid = $script:newPid
            return $script:newPid
        }
        WaitReady = {
            param($Context, $ProcessId)
            $script:events.Add("wait-ready:$ProcessId") | Out-Null
            if ($script:failAt -contains "ready" -and $ProcessId -eq $script:newPid) {
                throw "injected readiness failure"
            }
        }
        Restore = {
            param($Context, $Backup)
            $script:events.Add("restore") | Out-Null
            if ($script:failAt -contains "restore") { throw "injected restore failure" }
            if ($script:traceDestructiveRestore) {
                $script:events.Add("destructive:remove") | Out-Null
                $script:events.Add("destructive:copy") | Out-Null
                $script:events.Add("destructive:import") | Out-Null
            }
            $script:installedNew = $false
        }
        ValidateRestoreBackup = {
            param($Context, $Backup)
            $script:events.Add("validate-backup:$script:backupTamper") | Out-Null
            if (-not [string]::IsNullOrWhiteSpace($script:backupTamper)) {
                throw "injected backup manifest validation failure: $script:backupTamper"
            }
        }
        RestoreAppAndConfig = {
            param($Context, $Backup)
            $script:events.Add("restore-app-config") | Out-Null
            if ($script:failAt -contains "restore") { throw "injected restore failure" }
            $script:installedNew = $false
        }
        DeleteRegistryKey = {
            param($Context, $Backup)
            $script:events.Add("registry-delete:$($Context.RegistryKey)") | Out-Null
        }
        ImportRegistry = {
            param($Context, $Backup)
            $script:events.Add("registry-import") | Out-Null
        }
        VerifyRegistryRestore = {
            param($Context, $Backup)
            $script:events.Add("registry-verify") | Out-Null
        }
        VerifyRestoredState = {
            param($Context, $Backup)
            $script:events.Add("verify-restored-state") | Out-Null
        }
    }
}

function Assert-PreflightRejectsWithoutMutation {
    param(
        [hashtable]$Spec,
        [string]$Message
    )

    $operations = New-FakeOperations
    { Invoke-CcsmReinstallTransaction -Spec $Spec -Operations $operations -Simulation } | Should Throw $Message
    ($script:events -contains "backup") | Should Be $false
    (@($script:events | Where-Object { $_ -like "stop:*" -or $_ -like "unsafe-stop:*" }).Count) | Should Be 0
}

function Assert-TamperedBackupFailsClosed {
    param([string]$Tamper)

    $operations = New-FakeOperations -FailAt "install" -BackupTamper $Tamper -TraceDestructiveRestore
    $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

    $result.Status | Should Be "RollbackFailed"
    ($script:events -contains "validate-backup:$Tamper") | Should Be $true
    (@($script:events | Where-Object { $_ -like "destructive:*" }).Count) | Should Be 0
    ($script:events -contains "start:previous") | Should Be $true
    ($script:events -contains "wait-ready:6000") | Should Be $true
}

Describe "CCSwitchMulti transactional reinstall orchestration" {
    It "keeps condition polling internal instead of contaminating transaction output" {
        $script:conditionAttempts = 0
        $output = @(Wait-CcsmCondition -TimeoutSeconds 2 -Description "test condition" -Condition {
            $script:conditionAttempts++
            return $script:conditionAttempts -ge 2
        })

        $output.Count | Should Be 0
    }

    It "exposes a non-mutating plan with the complete transaction and rollback boundary" {
        $plan = Get-CcsmReinstallPlan

        ($plan.Forward -join ",") | Should Be "preflight,backup,stop-verified-pid,wait-port-release,quiescent-config-snapshot,uninstall-silent,install-silent,start-hidden,wait-listener-health,verify-version-hash-path"
        ($plan.Rollback -join ",") | Should Be "verify-and-stop-new-process,restore-app-config-registry,start-previous-hidden,wait-listener-health,verify-previous-runtime"
    }

    It "rejects a dot-dot backup path that only appears to be inside the transaction root" {
        Test-CcsmPathInside -Candidate "D:\ccsm-transaction-backups\txn-1\..\outside" `
            -Parent "D:\ccsm-transaction-backups\txn-1" | Should Be $false
    }

    It "runs every guard before the first mutating operation" {
        $operations = New-FakeOperations
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "Success"
        $script:events[0] | Should Be "log:preflight-ok"
        $script:events[1] | Should Be "backup"
        ($script:checks -contains "resolve:installer") | Should Be $true
        ($script:checks -contains "resolve:installed-executable") | Should Be $true
        ($script:checks -contains "resolve:install-directory") | Should Be $true
        ($script:checks -contains "resolve:uninstaller") | Should Be $true
        ($script:checks -contains "resolve:backup-root") | Should Be $true
        ($script:checks -contains "resolve:config") | Should Be $true
        ($script:checks -contains "hash:C:\artifacts\CCSwitchMulti-new-setup.exe") | Should Be $true
        ($script:checks -contains "hash:C:\Program Files\CCSwitchMulti\cc-switch.exe") | Should Be $true
        ($script:checks -contains "version:C:\Program Files\CCSwitchMulti\cc-switch.exe") | Should Be $true
        ($script:checks -contains "process-identity:4242") | Should Be $true
        ($script:checks -contains "listener:15721") | Should Be $true
    }

    It "refuses a listener owner mismatch without backup or process mutation" {
        $operations = New-FakeOperations -ListenerOwner 9999

        { Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation } |
            Should Throw "listener owner mismatch"
        ($script:events -contains "backup") | Should Be $false
        (@($script:events | Where-Object { $_ -like "stop:*" }).Count) | Should Be 0
    }

    It "rejects unsafe backup inputs before mutation" {
        $missingConfigSpec = New-TestSpec
        $missingConfigSpec.ConfigPaths = @()
        $operations = New-FakeOperations
        { Invoke-CcsmReinstallTransaction -Spec $missingConfigSpec -Operations $operations -Simulation } |
            Should Throw "at least one config path is required"
        ($script:events -contains "backup") | Should Be $false

        $embeddedInstallerSpec = New-TestSpec
        $embeddedInstallerSpec.InstallerPath = "C:\Program Files\CCSwitchMulti\new-setup.exe"
        $operations = New-FakeOperations
        { Invoke-CcsmReinstallTransaction -Spec $embeddedInstallerSpec -Operations $operations -Simulation } |
            Should Throw "installer must be outside the install directory"
        ($script:events -contains "backup") | Should Be $false
    }

    It "performs the forward transaction in exact order and verifies the new runtime" {
        $operations = New-FakeOperations
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation
        $mutations = @($script:events | Where-Object { $_ -notlike "log:*" })

        ($mutations -join ",") | Should Be "backup,stop:4242,wait-port-release,snapshot-config,verify-config-snapshot,uninstall,install,start:new,wait-ready:5000"
        ($script:checks -contains "process-path:5000") | Should Be $true
        ($script:checks -contains "health:http://127.0.0.1:15721/health") | Should Be $true
        $result.NewPid | Should Be 5000
    }

    It "rolls back app config and registry then verifies the previous runtime after a post-stop failure" {
        $operations = New-FakeOperations -FailAt "ready"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation
        $mutations = @($script:events | Where-Object { $_ -notlike "log:*" })

        $result.Status | Should Be "RolledBack"
        ($mutations -join ",") | Should Be "backup,stop:4242,wait-port-release,snapshot-config,verify-config-snapshot,uninstall,install,start:new,wait-ready:5000,stop:5000,validate-backup:,restore-app-config,registry-delete:HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti,registry-import,registry-verify,verify-restored-state,start:previous,wait-ready:6000"
        ($script:checks -contains "process-identity:5000") | Should Be $true
        ($script:checks -contains "process-path:6000") | Should Be $true
    }

    It "never terminates a newly launched process whose executable path cannot be verified" {
        $operations = New-FakeOperations -FailAt "ready" -NewProcessPath "C:\Windows\System32\notepad.exe"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RolledBack"
        ($script:events -contains "stop:5000") | Should Be $false
        ($script:events -contains "restore-app-config") | Should Be $true
    }

    It "reports rollback failure explicitly after attempting restore and restart" {
        $operations = New-FakeOperations -FailAt "install" -RollbackStartFails
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RollbackFailed"
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        $result.RollbackError | Should Match "rollback start failure"
    }

    It "continues restore and restart when the failed new process has already disappeared" {
        $operations = New-FakeOperations -FailAt "ready" -NewProcessPath "__missing__"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RolledBack"
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
    }

    It "still attempts to restart and verify the previous runtime when state restore fails" {
        $operations = New-FakeOperations -FailAt @("ready", "restore")
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RollbackFailed"
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        ($script:events -contains "wait-ready:6000") | Should Be $true
        $result.RollbackError | Should Match "restore failure"
    }

    It "does not attempt rollback when backup fails before the verified stop boundary" {
        $operations = New-FakeOperations -FailAt "backup"

        { Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation } |
            Should Throw "injected backup failure"
        ($script:events -contains "restore-app-config") | Should Be $false
        ($script:events -contains "start:previous") | Should Be $false
    }

    It "enables StrictMode Latest before defining the transaction surface" {
        & {
            Set-StrictMode -Off
            . $scriptPath
            { $undefinedTransactionVariable } | Should Throw
        }
    }

    It "continues restore, old start, readiness, and verification when transaction-failed logging throws" {
        $operations = New-FakeOperations -FailAt "ready" -FailLogEvent "transaction-failed"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RollbackFailed"
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        ($script:events -contains "wait-ready:6000") | Should Be $true
        ($script:checks -contains "process-path:6000") | Should Be $true
    }

    It "continues restore, old start, readiness, and verification when rollback-start logging throws" {
        $operations = New-FakeOperations -FailAt "ready" -FailLogEvent "rollback-start"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RollbackFailed"
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        ($script:events -contains "wait-ready:6000") | Should Be $true
        ($script:checks -contains "process-path:6000") | Should Be $true
    }

    It "returns RollbackFailed after recovery when rollback-success status logging throws" {
        $operations = New-FakeOperations -FailAt "ready" -FailLogEvent "rollback-success"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RollbackFailed"
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        ($script:events -contains "wait-ready:6000") | Should Be $true
        ($script:checks -contains "process-path:6000") | Should Be $true
    }

    It "returns RollbackFailed after recovery when rollback-failed status logging throws" {
        $operations = New-FakeOperations -FailAt @("ready", "restore") -FailLogEvent "rollback-failed"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RollbackFailed"
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        ($script:events -contains "wait-ready:6000") | Should Be $true
        ($script:checks -contains "process-path:6000") | Should Be $true
    }

    It "does not stop a PID whose path and StartTime changed after backup, and enters safe rollback" {
        $operations = New-FakeOperations -ChangeIdentityAfterBackup
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation

        $result.Status | Should Be "RolledBack"
        (@($script:events | Where-Object { $_ -like "unsafe-stop:*" }).Count) | Should Be 0
        (@($script:checks | Where-Object { $_ -eq "process-identity:4242" }).Count) | Should BeGreaterThan 1
        ($script:events -contains "restore-app-config") | Should Be $true
        ($script:events -contains "start:previous") | Should Be $true
        ($script:events -contains "wait-ready:6000") | Should Be $true
        ($script:checks -contains "process-path:6000") | Should Be $true
    }

    It "refuses an unhealthy old health endpoint before backup or stop" {
        $operations = New-FakeOperations -UnhealthyPreflight

        { Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation } |
            Should Throw "preflight health verification failed"
        ($script:events -contains "backup") | Should Be $false
        (@($script:events | Where-Object { $_ -like "stop:*" -or $_ -like "unsafe-stop:*" }).Count) | Should Be 0
    }

    It "rejects a volume-root config before mutation" {
        $spec = New-TestSpec
        $spec.ConfigPaths = @("C:\")
        Assert-PreflightRejectsWithoutMutation -Spec $spec -Message "config path must be a product-owned .cc-switch root"
    }

    It "rejects a user-profile config root before mutation" {
        $spec = New-TestSpec
        $spec.ConfigPaths = @("C:\Users\test")
        Assert-PreflightRejectsWithoutMutation -Spec $spec -Message "config path must be a product-owned .cc-switch root"
    }

    It "rejects a common user root config before mutation" {
        $spec = New-TestSpec
        $spec.ConfigPaths = @("C:\Users")
        Assert-PreflightRejectsWithoutMutation -Spec $spec -Message "config path must be a product-owned .cc-switch root"
    }

    It "rejects a non-product .cc-switch root before mutation" {
        $spec = New-TestSpec
        $spec.ConfigPaths = @("C:\Users\test\workspace\.cc-switch")
        Assert-PreflightRejectsWithoutMutation -Spec $spec -Message "config path must be a product-owned .cc-switch root"
    }

    It "rejects overlapping config paths before mutation" {
        $spec = New-TestSpec
        $spec.ConfigPaths = @("C:\Users\test\.cc-switch", "C:\Users\test\.cc-switch\state")
        Assert-PreflightRejectsWithoutMutation -Spec $spec -Message "config paths must be unique and non-overlapping"
    }

    It "rejects duplicate config paths before mutation" {
        $spec = New-TestSpec
        $spec.ConfigPaths = @("C:\Users\test\.cc-switch", "C:\Users\test\.cc-switch")
        Assert-PreflightRejectsWithoutMutation -Spec $spec -Message "config paths must be unique and non-overlapping"
    }

    It "rejects a foreign registry key before mutation" {
        $spec = New-TestSpec
        $spec.RegistryKey = "HKCU:\Software\Contoso\OtherProduct"
        Assert-PreflightRejectsWithoutMutation -Spec $spec -Message "registry key must be the exact CCSwitchMulti uninstall key"
    }

    It "rejects an app backup source that escapes the transaction root before destructive restore" {
        Assert-TamperedBackupFailsClosed -Tamper "app-escape"
    }

    It "rejects a config backup source that escapes the transaction root before destructive restore" {
        Assert-TamperedBackupFailsClosed -Tamper "config-escape"
    }

    It "rejects a registry backup source that escapes the transaction root before destructive restore" {
        Assert-TamperedBackupFailsClosed -Tamper "registry-escape"
    }

    It "rejects a backup manifest whose integrity hash was tampered before destructive restore" {
        Assert-TamperedBackupFailsClosed -Tamper "manifest-hash"
    }

    It "deletes the exact CCSwitchMulti registry key before import and verifies the restored key" {
        $operations = New-FakeOperations -FailAt "install"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation
        $registryEvents = @($script:events | Where-Object { $_ -like "registry-*" })

        $result.Status | Should Be "RolledBack"
        ($registryEvents -join ",") | Should Be "registry-delete:HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti,registry-import,registry-verify"
    }

    It "takes and verifies the config DB WAL SHM snapshot after release and verifies it after restore" {
        $operations = New-FakeOperations -FailAt "install"
        $result = Invoke-CcsmReinstallTransaction -Spec (New-TestSpec) -Operations $operations -Simulation
        $releaseIndex = $script:events.IndexOf("wait-port-release")
        $snapshotIndex = $script:events.IndexOf("snapshot-config")
        $uninstallIndex = $script:events.IndexOf("uninstall")
        $restoreVerificationIndex = $script:events.IndexOf("verify-restored-state")
        $previousStartIndex = $script:events.IndexOf("start:previous")

        $result.Status | Should Be "RolledBack"
        $snapshotIndex | Should BeGreaterThan $releaseIndex
        $snapshotIndex | Should BeLessThan $uninstallIndex
        ($script:events -contains "verify-config-snapshot") | Should Be $true
        $restoreVerificationIndex | Should BeLessThan $previousStartIndex
    }
}
