[CmdletBinding()]
param(
    [string]$InstallerPath = "",
    [string]$ExpectedInstallerHash = "",
    [string]$ExpectedCurrentVersion = "",
    [string]$ExpectedCurrentHash = "",
    [string]$ExpectedInstalledVersion = "",
    [string]$ExpectedInstalledHash = "",
    [int]$CurrentPid = 0,
    [string]$InstalledExecutable = "",
    [string]$InstallDirectory = "",
    [string]$UninstallExecutable = "",
    [string[]]$ConfigPath = @(),
    [string]$RegistryKey = "",
    [int]$Port = 0,
    [string]$HealthUri = "",
    [int]$TimeoutSeconds = 60,
    [string]$BackupRoot = "",
    [switch]$PlanOnly
)

$ErrorActionPreference = "Stop"

function Get-CcsmReinstallPlan {
    [CmdletBinding()]
    param()

    return [pscustomobject]@{
        Forward = @(
            "preflight",
            "backup",
            "stop-verified-pid",
            "wait-port-release",
            "uninstall-silent",
            "install-silent",
            "start-hidden",
            "wait-listener-health",
            "verify-version-hash-path"
        )
        Rollback = @(
            "verify-and-stop-new-process",
            "restore-app-config-registry",
            "start-previous-hidden",
            "wait-listener-health",
            "verify-previous-runtime"
        )
    }
}

function Test-CcsmSamePath {
    param([string]$Left, [string]$Right)

    $leftPath = $Left.TrimEnd([char[]]@([char]92, [char]47))
    $rightPath = $Right.TrimEnd([char[]]@([char]92, [char]47))
    return [string]::Equals($leftPath, $rightPath, [System.StringComparison]::OrdinalIgnoreCase)
}

function Test-CcsmPathInside {
    param([string]$Candidate, [string]$Parent)

    $candidatePath = $Candidate.TrimEnd([char[]]@([char]92, [char]47))
    $parentPath = $Parent.TrimEnd([char[]]@([char]92, [char]47))
    if (Test-CcsmSamePath -Left $candidatePath -Right $parentPath) {
        return $true
    }
    return $candidatePath.StartsWith(
        "$parentPath$([System.IO.Path]::DirectorySeparatorChar)",
        [System.StringComparison]::OrdinalIgnoreCase
    )
}

function Test-CcsmAbsolutePath {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return $false
    }
    return $Path -match '^(?:[A-Za-z]:[\\/]|\\\\[^\\/]+[\\/][^\\/]+(?:[\\/]|$))'
}

function Assert-CcsmHash {
    param([string]$Name, [string]$Value)

    if ($Value -notmatch '^[A-Fa-f0-9]{64}$') {
        throw "$Name must be an exact SHA-256 hash"
    }
}

function Assert-CcsmRegistryKey {
    param([string]$Key)

    if ($Key -notmatch '^(HKCU|HKLM):\\[^\\]+\\.+$') {
        throw "registry key must be a non-root HKCU or HKLM subkey: $Key"
    }
}

function Assert-CcsmRequiredOperations {
    param([hashtable]$Operations)

    $required = @(
        "ResolvePath", "TestPath", "GetFileHash", "GetFileVersion",
        "GetProcessPath", "GetListenerOwner", "GetHealth", "WriteLog",
        "Backup", "StopProcess", "WaitPortReleased", "RunUninstaller",
        "RunInstaller", "StartProcess", "WaitReady", "Restore"
    )
    foreach ($name in $required) {
        if (-not $Operations.ContainsKey($name) -or $Operations[$name] -isnot [scriptblock]) {
            throw "operation '$name' is required"
        }
    }
}

function Resolve-CcsmTransactionContext {
    param([hashtable]$Spec, [hashtable]$Operations)

    $requiredText = @(
        "InstallerPath", "ExpectedInstallerHash", "ExpectedCurrentVersion",
        "ExpectedCurrentHash", "ExpectedInstalledVersion", "ExpectedInstalledHash",
        "InstalledExecutable", "InstallDirectory", "UninstallExecutable",
        "RegistryKey", "HealthUri", "BackupRoot"
    )
    foreach ($name in $requiredText) {
        if (-not $Spec.ContainsKey($name) -or [string]::IsNullOrWhiteSpace([string]$Spec[$name])) {
            throw "spec field '$name' is required"
        }
    }
    foreach ($name in @("CurrentPid", "Port", "TimeoutSeconds")) {
        if (-not $Spec.ContainsKey($name) -or [int]$Spec[$name] -le 0) {
            throw "spec field '$name' must be greater than zero"
        }
    }

    Assert-CcsmHash -Name "ExpectedInstallerHash" -Value $Spec.ExpectedInstallerHash
    Assert-CcsmHash -Name "ExpectedCurrentHash" -Value $Spec.ExpectedCurrentHash
    Assert-CcsmHash -Name "ExpectedInstalledHash" -Value $Spec.ExpectedInstalledHash
    Assert-CcsmRegistryKey -Key $Spec.RegistryKey

    $pathFields = [ordered]@{
        InstallerPath       = "installer"
        InstalledExecutable = "installed-executable"
        InstallDirectory    = "install-directory"
        UninstallExecutable = "uninstaller"
        BackupRoot          = "backup-root"
    }
    $resolved = @{}
    foreach ($entry in $pathFields.GetEnumerator()) {
        $requested = [string]$Spec[$entry.Key]
        if (-not (Test-CcsmAbsolutePath $requested)) {
            throw "$($entry.Key) must be absolute: $requested"
        }
        $resolved[$entry.Key] = & $Operations.ResolvePath $requested $entry.Value
        if (-not (Test-CcsmAbsolutePath $resolved[$entry.Key])) {
            throw "$($entry.Key) did not resolve to an absolute path"
        }
        if (-not (& $Operations.TestPath $resolved[$entry.Key] $entry.Value)) {
            throw "$($entry.Key) does not exist: $($resolved[$entry.Key])"
        }
    }

    $configPaths = @()
    if (-not $Spec.ContainsKey("ConfigPaths") -or @($Spec.ConfigPaths).Count -eq 0) {
        throw "at least one config path is required"
    }
    foreach ($path in @($Spec.ConfigPaths)) {
        if (-not (Test-CcsmAbsolutePath $path)) {
            throw "every config path must be absolute"
        }
        $configResolved = & $Operations.ResolvePath $path "config"
        if (-not (Test-CcsmAbsolutePath $configResolved)) {
            throw "config path did not resolve to an absolute path"
        }
        if (-not (& $Operations.TestPath $configResolved "config")) {
            throw "config path does not exist: $configResolved"
        }
        $configPaths += $configResolved
    }

    $installDirectory = $resolved.InstallDirectory
    $installedExecutable = $resolved.InstalledExecutable
    $uninstallExecutable = $resolved.UninstallExecutable
    $backupRoot = $resolved.BackupRoot
    $installRoot = [System.IO.Path]::GetPathRoot($installDirectory)
    $installParent = Split-Path -Parent $installDirectory
    if ((Test-CcsmSamePath $installDirectory $installRoot) -or (Test-CcsmSamePath $installParent $installRoot)) {
        throw "install directory is too broad for transactional restore: $installDirectory"
    }
    if (-not (Test-CcsmSamePath (Split-Path -Parent $installedExecutable) $installDirectory)) {
        throw "installed executable must be an immediate child of the install directory"
    }
    if (-not (Test-CcsmSamePath (Split-Path -Parent $uninstallExecutable) $installDirectory)) {
        throw "uninstaller must be an immediate child of the install directory"
    }
    if (Test-CcsmPathInside $resolved.InstallerPath $installDirectory) {
        throw "installer must be outside the install directory"
    }
    if ((Test-CcsmPathInside $backupRoot $installDirectory) -or
        (Test-CcsmPathInside $installDirectory $backupRoot)) {
        throw "backup root must be external to and non-overlapping with the install directory"
    }
    foreach ($configResolved in $configPaths) {
        if ((Test-CcsmPathInside $backupRoot $configResolved) -or
            (Test-CcsmPathInside $configResolved $backupRoot) -or
            (Test-CcsmPathInside $configResolved $installDirectory) -or
            (Test-CcsmPathInside $installDirectory $configResolved)) {
            throw "config paths must not overlap the install directory or backup root: $configResolved"
        }
    }

    $health = [uri]$Spec.HealthUri
    if (-not $health.IsAbsoluteUri -or $health.Scheme -ne "http") {
        throw "health URI must be an absolute loopback HTTP URI"
    }
    if (@("127.0.0.1", "localhost", "::1") -notcontains $health.Host -or $health.Port -ne [int]$Spec.Port) {
        throw "health URI host and port must match the local listener"
    }
    if (-not [string]::IsNullOrEmpty($health.UserInfo)) {
        throw "health URI must not contain credentials"
    }

    $installerHash = & $Operations.GetFileHash $resolved.InstallerPath
    if (-not [string]::Equals($installerHash, $Spec.ExpectedInstallerHash, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "installer hash mismatch"
    }
    $currentHash = & $Operations.GetFileHash $installedExecutable
    if (-not [string]::Equals($currentHash, $Spec.ExpectedCurrentHash, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "current installed hash mismatch"
    }
    $currentVersion = & $Operations.GetFileVersion $installedExecutable
    if ($currentVersion -ne $Spec.ExpectedCurrentVersion) {
        throw "current installed version mismatch"
    }
    $processPath = & $Operations.GetProcessPath ([int]$Spec.CurrentPid)
    if (-not (Test-CcsmSamePath $processPath $installedExecutable)) {
        throw "current PID executable ownership mismatch"
    }
    $listenerOwner = & $Operations.GetListenerOwner ([int]$Spec.Port)
    if ($listenerOwner -ne [int]$Spec.CurrentPid) {
        throw "listener owner mismatch: actual=$listenerOwner expected=$($Spec.CurrentPid)"
    }

    $transactionId = "ccsm-{0}-{1}" -f (Get-Date -Format "yyyyMMdd-HHmmss"), ([guid]::NewGuid().ToString("N"))
    return [pscustomobject]@{
        InstallerPath            = $resolved.InstallerPath
        ExpectedInstallerHash    = $Spec.ExpectedInstallerHash.ToUpperInvariant()
        ExpectedCurrentVersion   = $Spec.ExpectedCurrentVersion
        ExpectedCurrentHash      = $Spec.ExpectedCurrentHash.ToUpperInvariant()
        ExpectedInstalledVersion = $Spec.ExpectedInstalledVersion
        ExpectedInstalledHash    = $Spec.ExpectedInstalledHash.ToUpperInvariant()
        CurrentPid               = [int]$Spec.CurrentPid
        InstalledExecutable      = $installedExecutable
        InstallDirectory         = $installDirectory
        UninstallExecutable      = $uninstallExecutable
        ConfigPaths              = $configPaths
        RegistryKey              = $Spec.RegistryKey
        Port                     = [int]$Spec.Port
        HealthUri                = $health.AbsoluteUri
        TimeoutSeconds           = [int]$Spec.TimeoutSeconds
        BackupRoot               = $backupRoot
        TransactionId            = $transactionId
        TransactionRoot          = (Join-Path $backupRoot $transactionId)
        LogPath                  = (Join-Path (Join-Path $backupRoot $transactionId) "transaction.jsonl")
    }
}

function Assert-CcsmRuntime {
    param(
        $Context,
        [hashtable]$Operations,
        [int]$ProcessId,
        [string]$ExpectedVersion,
        [string]$ExpectedHash,
        [string]$Label
    )

    $processPath = & $Operations.GetProcessPath $ProcessId
    if (-not (Test-CcsmSamePath $processPath $Context.InstalledExecutable)) {
        throw "$Label process path mismatch"
    }
    $listenerOwner = & $Operations.GetListenerOwner $Context.Port
    if ($listenerOwner -ne $ProcessId) {
        throw "$Label listener owner mismatch"
    }
    $health = & $Operations.GetHealth $Context.HealthUri
    if (-not $health.Healthy -or [int]$health.StatusCode -lt 200 -or [int]$health.StatusCode -ge 300) {
        throw "$Label health verification failed"
    }
    $version = & $Operations.GetFileVersion $Context.InstalledExecutable
    if ($version -ne $ExpectedVersion) {
        throw "$Label version mismatch"
    }
    $hash = & $Operations.GetFileHash $Context.InstalledExecutable
    if (-not [string]::Equals($hash, $ExpectedHash, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label hash mismatch"
    }
}

function Invoke-CcsmReinstallTransaction {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][hashtable]$Spec,
        [hashtable]$Operations,
        [switch]$Simulation
    )

    if ($Simulation -and $null -eq $Operations) {
        throw "simulation requires injected operations"
    }
    if (-not $Simulation -and $null -ne $Operations) {
        throw "injected operations are allowed only with -Simulation"
    }
    if ($null -eq $Operations) {
        $Operations = New-CcsmRealOperations
    }
    Assert-CcsmRequiredOperations -Operations $Operations
    $context = Resolve-CcsmTransactionContext -Spec $Spec -Operations $Operations
    & $Operations.WriteLog $context "info" "preflight-ok" @{ CurrentPid = $context.CurrentPid }

    $backup = $null
    $rollbackRequired = $false
    $newPid = $null
    try {
        $backup = & $Operations.Backup $context
        & $Operations.WriteLog $context "info" "backup-ok" @{ BackupPath = $backup.Path }

        $rollbackRequired = $true
        & $Operations.StopProcess $context $context.CurrentPid
        & $Operations.WaitPortReleased $context
        & $Operations.RunUninstaller $context
        & $Operations.RunInstaller $context
        $newPid = & $Operations.StartProcess $context "new"
        & $Operations.WaitReady $context $newPid
        Assert-CcsmRuntime -Context $context -Operations $Operations -ProcessId $newPid `
            -ExpectedVersion $context.ExpectedInstalledVersion -ExpectedHash $context.ExpectedInstalledHash -Label "new runtime"
        & $Operations.WriteLog $context "info" "transaction-success" @{ NewPid = $newPid }
        return [pscustomobject]@{
            Status = "Success"
            TransactionId = $context.TransactionId
            BackupPath = $backup.Path
            NewPid = $newPid
            Error = $null
            RollbackError = $null
        }
    } catch {
        $transactionError = $_.Exception.Message
        & $Operations.WriteLog $context "error" "transaction-failed" @{ Error = $transactionError }
        if (-not $rollbackRequired) {
            throw
        }

        & $Operations.WriteLog $context "warning" "rollback-start" @{ Error = $transactionError }
        $rollbackErrors = New-Object System.Collections.Generic.List[string]
        if ($null -ne $newPid) {
            $newPath = $null
            try {
                $newPath = & $Operations.GetProcessPath $newPid
            } catch {
                & $Operations.WriteLog $context "warning" "rollback-new-process-not-verifiable" @{
                    ProcessId = $newPid
                    Error = $_.Exception.Message
                }
            }
            if ($null -ne $newPath -and (Test-CcsmSamePath $newPath $context.InstalledExecutable)) {
                try {
                    & $Operations.StopProcess $context $newPid
                } catch {
                    $rollbackErrors.Add("stop new process: $($_.Exception.Message)") | Out-Null
                }
            } elseif ($null -ne $newPath) {
                & $Operations.WriteLog $context "warning" "rollback-skip-unverified-process" @{ ProcessId = $newPid }
            }
        }

        try {
            & $Operations.Restore $context $backup
        } catch {
            $rollbackErrors.Add("restore state: $($_.Exception.Message)") | Out-Null
        }

        $previousPid = $null
        try {
            $previousPid = & $Operations.StartProcess $context "previous"
        } catch {
            $rollbackErrors.Add("start previous runtime: $($_.Exception.Message)") | Out-Null
        }
        if ($null -ne $previousPid) {
            try {
                & $Operations.WaitReady $context $previousPid
                Assert-CcsmRuntime -Context $context -Operations $Operations -ProcessId $previousPid `
                    -ExpectedVersion $context.ExpectedCurrentVersion -ExpectedHash $context.ExpectedCurrentHash -Label "rollback runtime"
            } catch {
                $rollbackErrors.Add("verify previous runtime: $($_.Exception.Message)") | Out-Null
            }
        }

        if ($rollbackErrors.Count -eq 0) {
            & $Operations.WriteLog $context "info" "rollback-success" @{ PreviousPid = $previousPid }
            return [pscustomobject]@{
                Status = "RolledBack"
                TransactionId = $context.TransactionId
                BackupPath = $backup.Path
                NewPid = $newPid
                Error = $transactionError
                RollbackError = $null
            }
        }
        $rollbackError = $rollbackErrors -join "; "
        & $Operations.WriteLog $context "error" "rollback-failed" @{
            Error = $transactionError
            RollbackError = $rollbackError
        }
        return [pscustomobject]@{
            Status = "RollbackFailed"
            TransactionId = $context.TransactionId
            BackupPath = $backup.Path
            NewPid = $newPid
            Error = $transactionError
            RollbackError = $rollbackError
        }
    }
}

function Wait-CcsmCondition {
    param(
        [scriptblock]$Condition,
        [int]$TimeoutSeconds,
        [string]$Description
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $signal = New-Object System.Threading.ManualResetEventSlim($false)
    try {
        while ([DateTime]::UtcNow -lt $deadline) {
            if (& $Condition) {
                return
            }
            $remainingMilliseconds = [int][Math]::Max(1, ($deadline - [DateTime]::UtcNow).TotalMilliseconds)
            [void]$signal.Wait([Math]::Min(200, $remainingMilliseconds))
        }
    } finally {
        $signal.Dispose()
    }
    throw "timed out waiting for $Description"
}

function ConvertTo-CcsmNativeRegistryPath {
    param([string]$RegistryKey)

    if ($RegistryKey.StartsWith("HKCU:\", [System.StringComparison]::OrdinalIgnoreCase)) {
        return "HKCU\" + $RegistryKey.Substring(6)
    }
    if ($RegistryKey.StartsWith("HKLM:\", [System.StringComparison]::OrdinalIgnoreCase)) {
        return "HKLM\" + $RegistryKey.Substring(6)
    }
    throw "unsupported registry key: $RegistryKey"
}

function Assert-CcsmRestoreBoundary {
    param($Context, [string]$BackupPath)

    if (-not (Test-CcsmPathInside $BackupPath $Context.BackupRoot) -or
        (Test-CcsmSamePath $BackupPath $Context.BackupRoot) -or
        -not (Test-CcsmSamePath $BackupPath $Context.TransactionRoot)) {
        throw "backup path escaped the validated transaction boundary"
    }
    if (-not (Test-CcsmPathInside $Context.InstalledExecutable $Context.InstallDirectory)) {
        throw "install restore boundary is invalid"
    }
}

function New-CcsmRealOperations {
    [CmdletBinding()]
    param()

    $operations = @{}
    $operations.ResolvePath = {
        param($Path, $Kind)
        $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
        if ($resolved.Provider.Name -ne "FileSystem") {
            throw "$Kind path is not a filesystem path: $Path"
        }
        return $resolved.ProviderPath
    }
    $operations.TestPath = {
        param($Path, $Kind)
        if (-not (Test-Path -LiteralPath $Path)) { return $false }
        $item = Get-Item -LiteralPath $Path -Force
        if (@("installer", "installed-executable", "uninstaller") -contains $Kind) {
            return -not $item.PSIsContainer
        }
        if (@("install-directory", "backup-root") -contains $Kind) {
            return $item.PSIsContainer
        }
        return $true
    }
    $operations.GetFileHash = {
        param($Path)
        return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    }
    $operations.GetFileVersion = {
        param($Path)
        $info = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
        if (-not [string]::IsNullOrWhiteSpace($info.ProductVersion)) { return $info.ProductVersion }
        return $info.FileVersion
    }
    $operations.GetProcessPath = {
        param($ProcessId)
        $process = Get-Process -Id $ProcessId -ErrorAction Stop
        return $process.MainModule.FileName
    }
    $operations.GetListenerOwner = {
        param($Port)
        $owners = @(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty OwningProcess -Unique)
        if ($owners.Count -gt 1) { throw "multiple processes own listener port $Port" }
        if ($owners.Count -eq 0) { return $null }
        return [int]$owners[0]
    }
    $operations.GetHealth = {
        param($Uri)
        try {
            $response = Invoke-WebRequest -Uri $Uri -UseBasicParsing -TimeoutSec 2
            return @{ StatusCode = [int]$response.StatusCode; Healthy = $true }
        } catch {
            return @{ StatusCode = 0; Healthy = $false }
        }
    }
    $operations.WriteLog = {
        param($Context, $Level, $Event, $Detail)
        if (-not (Test-Path -LiteralPath $Context.TransactionRoot)) {
            New-Item -ItemType Directory -Path $Context.TransactionRoot -ErrorAction Stop | Out-Null
        }
        $entry = [ordered]@{
            timestamp = (Get-Date).ToUniversalTime().ToString("o")
            transactionId = $Context.TransactionId
            level = $Level
            event = $Event
            detail = $Detail
        }
        Add-Content -LiteralPath $Context.LogPath -Value ($entry | ConvertTo-Json -Compress -Depth 6) -Encoding UTF8
    }
    $operations.Backup = {
        param($Context)
        Assert-CcsmRestoreBoundary -Context $Context -BackupPath $Context.TransactionRoot
        $appBackup = Join-Path $Context.TransactionRoot "app"
        Copy-Item -LiteralPath $Context.InstallDirectory -Destination $appBackup -Recurse -Force -ErrorAction Stop

        $configBackups = @()
        $configRoot = Join-Path $Context.TransactionRoot "config"
        if ($Context.ConfigPaths.Count -gt 0) {
            New-Item -ItemType Directory -Path $configRoot -ErrorAction Stop | Out-Null
        }
        for ($index = 0; $index -lt $Context.ConfigPaths.Count; $index++) {
            $destination = Join-Path $configRoot ([string]$index)
            Copy-Item -LiteralPath $Context.ConfigPaths[$index] -Destination $destination -Recurse -Force -ErrorAction Stop
            $configBackups += @{ Source = $Context.ConfigPaths[$index]; Backup = $destination }
        }

        $registryExisted = Test-Path -LiteralPath $Context.RegistryKey
        $registryFile = Join-Path $Context.TransactionRoot "registry.reg"
        if ($registryExisted) {
            $nativeKey = ConvertTo-CcsmNativeRegistryPath $Context.RegistryKey
            & reg.exe export $nativeKey $registryFile /y | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "registry export failed with exit code $LASTEXITCODE" }
        }
        $manifest = [ordered]@{
            TransactionId = $Context.TransactionId
            InstallDirectory = $Context.InstallDirectory
            AppBackup = $appBackup
            ConfigBackups = $configBackups
            RegistryKey = $Context.RegistryKey
            RegistryExisted = $registryExisted
            RegistryFile = $registryFile
        }
        $manifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $Context.TransactionRoot "backup-manifest.json") -Encoding UTF8
        return [pscustomobject]@{ Path = $Context.TransactionRoot; Manifest = $manifest }
    }
    $operations.StopProcess = {
        param($Context, $ProcessId)
        Stop-Process -Id $ProcessId -Force -ErrorAction Stop
        Wait-Process -Id $ProcessId -Timeout $Context.TimeoutSeconds -ErrorAction SilentlyContinue
        if (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue) {
            throw "process $ProcessId did not exit"
        }
    }
    $operations.WaitPortReleased = {
        param($Context)
        Wait-CcsmCondition -TimeoutSeconds $Context.TimeoutSeconds -Description "port $($Context.Port) release" -Condition {
            $null -eq (& $operations.GetListenerOwner $Context.Port)
        }
    }
    $operations.RunUninstaller = {
        param($Context)
        $process = Start-Process -FilePath $Context.UninstallExecutable `
            -ArgumentList @("/S", "_?=$($Context.InstallDirectory)") -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "uninstaller failed with exit code $($process.ExitCode)" }
    }
    $operations.RunInstaller = {
        param($Context)
        $process = Start-Process -FilePath $Context.InstallerPath -ArgumentList @("/S") -WindowStyle Hidden -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "installer failed with exit code $($process.ExitCode)" }
    }
    $operations.StartProcess = {
        param($Context, $Mode)
        $process = Start-Process -FilePath $Context.InstalledExecutable -WindowStyle Hidden -PassThru
        return [int]$process.Id
    }
    $operations.WaitReady = {
        param($Context, $ProcessId)
        Wait-CcsmCondition -TimeoutSeconds $Context.TimeoutSeconds -Description "CCSwitchMulti listener and health" -Condition {
            $owner = & $operations.GetListenerOwner $Context.Port
            if ($owner -ne $ProcessId) { return $false }
            $healthResult = & $operations.GetHealth $Context.HealthUri
            return $healthResult.Healthy -and [int]$healthResult.StatusCode -ge 200 -and [int]$healthResult.StatusCode -lt 300
        }
    }
    $operations.Restore = {
        param($Context, $Backup)
        Assert-CcsmRestoreBoundary -Context $Context -BackupPath $Backup.Path
        $manifestPath = Join-Path $Backup.Path "backup-manifest.json"
        if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
            throw "backup manifest is missing"
        }
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        if ($manifest.TransactionId -ne $Context.TransactionId -or
            -not (Test-CcsmSamePath $manifest.InstallDirectory $Context.InstallDirectory) -or
            -not (Test-Path -LiteralPath $manifest.AppBackup -PathType Container)) {
            throw "backup manifest does not match the validated transaction"
        }

        if (Test-Path -LiteralPath $Context.InstallDirectory) {
            Remove-Item -LiteralPath $Context.InstallDirectory -Recurse -Force -ErrorAction Stop
        }
        Copy-Item -LiteralPath $manifest.AppBackup -Destination $Context.InstallDirectory -Recurse -Force -ErrorAction Stop

        foreach ($configBackup in @($manifest.ConfigBackups)) {
            $expectedConfig = @($Context.ConfigPaths | Where-Object { Test-CcsmSamePath $_ $configBackup.Source })
            if ($expectedConfig.Count -ne 1 -or -not (Test-Path -LiteralPath $configBackup.Backup)) {
                throw "config backup escaped the validated restore boundary"
            }
            if (Test-Path -LiteralPath $configBackup.Source) {
                Remove-Item -LiteralPath $configBackup.Source -Recurse -Force -ErrorAction Stop
            }
            Copy-Item -LiteralPath $configBackup.Backup -Destination $configBackup.Source -Recurse -Force -ErrorAction Stop
        }

        if ($manifest.RegistryExisted) {
            if (-not (Test-Path -LiteralPath $manifest.RegistryFile -PathType Leaf)) {
                throw "registry backup is missing"
            }
            & reg.exe import $manifest.RegistryFile | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "registry import failed with exit code $LASTEXITCODE" }
        } elseif (Test-Path -LiteralPath $Context.RegistryKey) {
            Assert-CcsmRegistryKey -Key $Context.RegistryKey
            Remove-Item -LiteralPath $Context.RegistryKey -Recurse -Force -ErrorAction Stop
        }
    }
    return $operations
}

if ($MyInvocation.InvocationName -ne '.') {
    if ($PlanOnly) {
        Get-CcsmReinstallPlan | ConvertTo-Json -Depth 4
        return
    }
    $spec = @{
        InstallerPath = $InstallerPath
        ExpectedInstallerHash = $ExpectedInstallerHash
        ExpectedCurrentVersion = $ExpectedCurrentVersion
        ExpectedCurrentHash = $ExpectedCurrentHash
        ExpectedInstalledVersion = $ExpectedInstalledVersion
        ExpectedInstalledHash = $ExpectedInstalledHash
        CurrentPid = $CurrentPid
        InstalledExecutable = $InstalledExecutable
        InstallDirectory = $InstallDirectory
        UninstallExecutable = $UninstallExecutable
        ConfigPaths = $ConfigPath
        RegistryKey = $RegistryKey
        Port = $Port
        HealthUri = $HealthUri
        TimeoutSeconds = $TimeoutSeconds
        BackupRoot = $BackupRoot
    }
    $result = Invoke-CcsmReinstallTransaction -Spec $spec
    $result | ConvertTo-Json -Depth 6
    if ($result.Status -ne "Success") { exit 1 }
}
