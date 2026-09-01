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
Set-StrictMode -Version Latest

$script:CcsmUninstallRegistryKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\CCSwitchMulti"

function Get-CcsmSha256 {
    param([Parameter(Mandatory = $true)][string]$LiteralPath)

    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $stream = [System.IO.File]::OpenRead($LiteralPath)
        try {
            return [System.BitConverter]::ToString($sha256.ComputeHash($stream)).Replace("-", "")
        } finally {
            $stream.Dispose()
        }
    } finally {
        $sha256.Dispose()
    }
}

function Get-CcsmReinstallPlan {
    [CmdletBinding()]
    param()

    return [pscustomobject]@{
        Forward = @(
            "preflight",
            "backup",
            "stop-verified-pid",
            "wait-port-release",
            "retire-owned-run-marker",
            "quiescent-config-snapshot",
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

function ConvertTo-CcsmCanonicalPath {
    param([string]$Path)

    if ([string]::IsNullOrWhiteSpace($Path)) { throw "path must not be empty" }
    return [System.IO.Path]::GetFullPath($Path).TrimEnd([char[]]@([char]92, [char]47))
}

function ConvertTo-CcsmHostLocalPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    $canonical = ConvertTo-CcsmCanonicalPath -Path $Path
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) { return $canonical }
    $localAppData = ConvertTo-CcsmCanonicalPath -Path $env:LOCALAPPDATA
    $escaped = [regex]::Escape($localAppData)
    $pattern = "^$escaped\\Packages\\OpenAI\.Codex_[^\\]+\\LocalCache\\Local(?:\\(?<relative>.*))?$"
    $match = [regex]::Match($canonical, $pattern, [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
    if (-not $match.Success) { return $canonical }
    $relative = [string]$match.Groups["relative"].Value
    if ([string]::IsNullOrWhiteSpace($relative)) { return $localAppData }
    return ConvertTo-CcsmCanonicalPath -Path (Join-Path $localAppData $relative)
}

function Test-CcsmSamePath {
    param([string]$Left, [string]$Right)

    $leftPath = ConvertTo-CcsmHostLocalPath -Path $Left
    $rightPath = ConvertTo-CcsmHostLocalPath -Path $Right
    return [string]::Equals($leftPath, $rightPath, [System.StringComparison]::OrdinalIgnoreCase)
}

function Test-CcsmPathInside {
    param([string]$Candidate, [string]$Parent)

    $candidatePath = ConvertTo-CcsmCanonicalPath -Path $Candidate
    $parentPath = ConvertTo-CcsmCanonicalPath -Path $Parent
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

    if (-not [string]::Equals($Key, $script:CcsmUninstallRegistryKey, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "registry key must be the exact CCSwitchMulti uninstall key: $script:CcsmUninstallRegistryKey"
    }
}

function Test-CcsmStrictDescendant {
    param([string]$Candidate, [string]$Parent)

    return (Test-CcsmPathInside -Candidate $Candidate -Parent $Parent) -and
        -not (Test-CcsmSamePath -Left $Candidate -Right $Parent)
}

function Remove-CcsmVerifiedRunMarker {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string]$ConfigPath,
        [Parameter(Mandatory = $true)]$ExpectedIdentity,
        [Parameter(Mandatory = $true)][string]$ExpectedVersion
    )

    $configRoot = [System.IO.Path]::GetFullPath($ConfigPath)
    $markerPath = [System.IO.Path]::GetFullPath((Join-Path $configRoot "logs\app-run-marker.json"))
    if (-not (Test-CcsmStrictDescendant -Candidate $markerPath -Parent $configRoot)) {
        throw "run marker escaped the validated config root"
    }
    if (-not (Test-Path -LiteralPath $markerPath -PathType Leaf)) { return }

    $marker = [System.IO.File]::ReadAllText($markerPath, [System.Text.Encoding]::UTF8) | ConvertFrom-Json
    foreach ($field in @("pid", "version", "executablePath", "processStartedAtTicks")) {
        if ($marker.PSObject.Properties.Name -notcontains $field) {
            throw "run marker is missing required identity field '$field'"
        }
    }

    $expectedPid = [int]$ExpectedIdentity.ProcessId
    $expectedPath = [string]$ExpectedIdentity.Path
    $expectedStartTicks = [datetimeoffset]::Parse(
        [string]$ExpectedIdentity.StartTime,
        [System.Globalization.CultureInfo]::InvariantCulture,
        [System.Globalization.DateTimeStyles]::RoundtripKind
    ).UtcDateTime.ToFileTimeUtc()
    if ([int]$marker.pid -ne $expectedPid -or
        -not [string]::Equals([string]$marker.version, $ExpectedVersion, [System.StringComparison]::Ordinal) -or
        -not (Test-CcsmSamePath -Left ([string]$marker.executablePath) -Right $expectedPath) -or
        [int64]$marker.processStartedAtTicks -ne [int64]$expectedStartTicks) {
        throw "run marker does not belong to the exact stopped CCSwitchMulti process"
    }

    Remove-Item -LiteralPath $markerPath -Force -ErrorAction Stop
}

function Get-CcsmExistingFileSystemItem {
    param([string]$Path)

    try {
        return Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    } catch {
        if ($_.CategoryInfo.Category -eq [System.Management.Automation.ErrorCategory]::ObjectNotFound) {
            return $null
        }
        throw "cannot inspect filesystem boundary: $Path"
    }
}

function Assert-CcsmNoReparseTree {
    param($Item, [string]$Purpose)

    if (($Item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "reparse"
    }
    if (-not $Item.PSIsContainer) { return }
    foreach ($child in @(Get-ChildItem -LiteralPath $Item.FullName -Force -ErrorAction Stop)) {
        Assert-CcsmNoReparseTree -Item $child -Purpose $Purpose
    }
}

function Assert-CcsmNoReparseBoundary {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string[]]$Path,
        [Parameter(Mandatory = $true)][string]$Purpose
    )

    foreach ($candidate in @($Path)) {
        if ([string]::IsNullOrWhiteSpace($candidate)) { throw "reparse" }
        $current = [System.IO.Path]::GetFullPath($candidate)
        $candidatePath = $current
        while ($true) {
            $item = Get-CcsmExistingFileSystemItem -Path $current
            if ($null -ne $item) {
                if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                    throw "reparse"
                }
                if ([string]::Equals($current, $candidatePath, [System.StringComparison]::OrdinalIgnoreCase)) {
                    Assert-CcsmNoReparseTree -Item $item -Purpose $Purpose
                }
            }
            $parent = [System.IO.Directory]::GetParent($current)
            if ($null -eq $parent -or [string]::Equals($parent.FullName, $current, [System.StringComparison]::OrdinalIgnoreCase)) {
                break
            }
            $current = $parent.FullName
        }
    }
}

function Get-CcsmTransactionExitCode {
    param($Result)

    if ($null -eq $Result) { return 1 }
    $statusProperty = $Result.PSObject.Properties["Status"]
    $errorProperty = $Result.PSObject.Properties["Error"]
    $rollbackErrorProperty = $Result.PSObject.Properties["RollbackError"]
    if ($null -eq $statusProperty -or $null -eq $errorProperty -or $null -eq $rollbackErrorProperty) {
        return 1
    }
    if ([string]::Equals([string]$statusProperty.Value, "Success", [System.StringComparison]::Ordinal) -and
        [string]::IsNullOrWhiteSpace([string]$errorProperty.Value) -and
        [string]::IsNullOrWhiteSpace([string]$rollbackErrorProperty.Value)) {
        return 0
    }
    return 1
}

function Assert-CcsmNoReparsePathBoundary {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string[]]$Path,
        [Parameter(Mandatory = $true)][string]$Purpose
    )

    foreach ($candidate in @($Path)) {
        if ([string]::IsNullOrWhiteSpace($candidate)) { throw "reparse" }
        $current = [System.IO.Path]::GetFullPath($candidate)
        while ($true) {
            $item = Get-CcsmExistingFileSystemItem -Path $current
            if ($null -ne $item -and
                ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "reparse"
            }
            $parent = [System.IO.Directory]::GetParent($current)
            if ($null -eq $parent -or [string]::Equals($parent.FullName, $current, [System.StringComparison]::OrdinalIgnoreCase)) {
                break
            }
            $current = $parent.FullName
        }
    }
}

function Remove-CcsmDirectoryTree {
    param([string]$Path, [string]$Purpose)

    if (Test-Path -LiteralPath $Path) {
        Assert-CcsmNoReparseBoundary -Path $Path -Purpose $Purpose
        Remove-Item -LiteralPath $Path -Recurse -Force -ErrorAction Stop
    }
}

function Assert-CcsmProductInstallBoundary {
    param(
        [string]$InstallDirectory,
        [string]$InstalledExecutable,
        [string]$UninstallExecutable
    )

    $root = [System.IO.Path]::GetPathRoot($InstallDirectory)
    $parent = Split-Path -Parent $InstallDirectory
    if ((Test-CcsmSamePath -Left $InstallDirectory -Right $root) -or
        (Test-CcsmSamePath -Left $parent -Right $root)) {
        throw "install directory is too broad for transactional restore: $InstallDirectory"
    }
    if (-not [string]::Equals((Split-Path -Leaf $InstallDirectory), "CCSwitchMulti", [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "install directory must be the product-owned CCSwitchMulti directory"
    }
    if (-not (Test-CcsmSamePath -Left (Split-Path -Parent $InstalledExecutable) -Right $InstallDirectory) -or
        -not [string]::Equals((Split-Path -Leaf $InstalledExecutable), "cc-switch.exe", [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "installed executable must be the product-owned cc-switch.exe immediate child"
    }
    if (-not (Test-CcsmSamePath -Left (Split-Path -Parent $UninstallExecutable) -Right $InstallDirectory)) {
        throw "uninstaller must be an immediate child of the install directory"
    }
}

function Test-CcsmProductConfigRoot {
    param([string]$Path)

    return $Path -match '^[A-Za-z]:\\Users\\[^\\]+\\\.cc-switch$'
}

function Assert-CcsmProductConfigBoundary {
    param([string[]]$ConfigPaths)

    for ($leftIndex = 0; $leftIndex -lt $ConfigPaths.Count; $leftIndex++) {
        for ($rightIndex = $leftIndex + 1; $rightIndex -lt $ConfigPaths.Count; $rightIndex++) {
            if ((Test-CcsmPathInside -Candidate $ConfigPaths[$leftIndex] -Parent $ConfigPaths[$rightIndex]) -or
                (Test-CcsmPathInside -Candidate $ConfigPaths[$rightIndex] -Parent $ConfigPaths[$leftIndex])) {
                throw "config paths must be unique and non-overlapping"
            }
        }
    }
    foreach ($configPath in $ConfigPaths) {
        if (-not (Test-CcsmProductConfigRoot -Path $configPath)) {
            throw "config path must be a product-owned .cc-switch root: $configPath"
        }
    }
}

function Assert-CcsmHealthyResponse {
    param($Health, [string]$Label)

    if ($null -eq $Health -or -not $Health.Healthy -or [int]$Health.StatusCode -lt 200 -or [int]$Health.StatusCode -ge 300) {
        throw "$Label health verification failed"
    }
}

function Assert-CcsmRequiredOperations {
    param([hashtable]$Operations)

    $required = @(
        "ResolvePath", "TestPath", "GetFileHash", "GetFileVersion",
        "GetProcessPath", "GetProcessIdentity", "GetListenerOwner", "GetHealth", "WriteLog",
        "Backup", "StopVerifiedProcess", "WaitPortReleased", "RetireRunMarker", "SnapshotConfig", "VerifyConfigSnapshot",
        "RunUninstaller", "RunInstaller", "StartProcess", "WaitReady", "ValidateRestoreBackup",
        "RestoreAppAndConfig", "DeleteRegistryKey", "ImportRegistry", "VerifyRegistryRestore", "VerifyRestoredState"
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
    Assert-CcsmProductInstallBoundary -InstallDirectory $installDirectory -InstalledExecutable $installedExecutable -UninstallExecutable $uninstallExecutable
    Assert-CcsmProductConfigBoundary -ConfigPaths $configPaths
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
    Assert-CcsmNoReparseBoundary -Path (@(
        $resolved.InstallerPath, $installedExecutable, $installDirectory, $uninstallExecutable, $backupRoot
    ) + @($configPaths)) -Purpose "transaction preflight"

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
    $processIdentity = & $Operations.GetProcessIdentity ([int]$Spec.CurrentPid)
    if ($null -eq $processIdentity -or [int]$processIdentity.ProcessId -ne [int]$Spec.CurrentPid -or
        -not (Test-CcsmSamePath -Left ([string]$processIdentity.Path) -Right $installedExecutable) -or
        [string]::IsNullOrWhiteSpace([string]$processIdentity.StartTime)) {
        throw "current PID executable ownership mismatch"
    }
    $listenerOwner = & $Operations.GetListenerOwner ([int]$Spec.Port)
    if ($listenerOwner -ne [int]$Spec.CurrentPid) {
        throw "listener owner mismatch: actual=$listenerOwner expected=$($Spec.CurrentPid)"
    }
    Assert-CcsmHealthyResponse -Health (& $Operations.GetHealth $health.AbsoluteUri) -Label "preflight"

    $transactionId = "ccsm-{0}-{1}" -f (Get-Date -Format "yyyyMMdd-HHmmss"), ([guid]::NewGuid().ToString("N"))
    return [pscustomobject]@{
        InstallerPath            = $resolved.InstallerPath
        ExpectedInstallerHash    = $Spec.ExpectedInstallerHash.ToUpperInvariant()
        ExpectedCurrentVersion   = $Spec.ExpectedCurrentVersion
        ExpectedCurrentHash      = $Spec.ExpectedCurrentHash.ToUpperInvariant()
        ExpectedInstalledVersion = $Spec.ExpectedInstalledVersion
        ExpectedInstalledHash    = $Spec.ExpectedInstalledHash.ToUpperInvariant()
        CurrentPid               = [int]$Spec.CurrentPid
        CurrentProcessIdentity   = $processIdentity
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
    Assert-CcsmHealthyResponse -Health (& $Operations.GetHealth $Context.HealthUri) -Label $Label
    $version = & $Operations.GetFileVersion $Context.InstalledExecutable
    if ($version -ne $ExpectedVersion) {
        throw "$Label version mismatch"
    }
    $hash = & $Operations.GetFileHash $Context.InstalledExecutable
    if (-not [string]::Equals($hash, $ExpectedHash, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label hash mismatch: actual=$hash expected=$ExpectedHash"
    }
}

function Test-CcsmSameProcessIdentity {
    param($Expected, $Actual)

    if ($null -eq $Expected -or $null -eq $Actual) { return $false }
    if ([int]$Expected.ProcessId -ne [int]$Actual.ProcessId) { return $false }
    if (-not (Test-CcsmSamePath -Left ([string]$Expected.Path) -Right ([string]$Actual.Path))) { return $false }
    return [string]::Equals([string]$Expected.StartTime, [string]$Actual.StartTime, [System.StringComparison]::Ordinal)
}

function Resolve-CcsmReplacementListenerAction {
    param($Context, $ListenerIdentity)

    if ($null -eq $ListenerIdentity) { return "released" }
    if ([int]$ListenerIdentity.ProcessId -eq [int]$Context.CurrentPid) { return "wait" }
    if (-not (Test-CcsmSamePath -Left ([string]$ListenerIdentity.Path) -Right ([string]$Context.InstalledExecutable))) {
        throw "foreign process owns listener port after verified stop: pid=$($ListenerIdentity.ProcessId) path=$($ListenerIdentity.Path)"
    }
    if ($null -eq $ListenerIdentity.Handle) {
        throw "same-product replacement listener has no retained process handle"
    }
    return "stop"
}

function Resolve-CcsmExistingRuntimeProcessId {
    param(
        $Context,
        $ListenerIdentity,
        [string]$ExpectedVersion,
        [string]$ExpectedHash,
        [string]$ActualVersion,
        [string]$ActualHash,
        $Health
    )

    if ($null -eq $ListenerIdentity -or
        -not (Test-CcsmSamePath -Left ([string]$ListenerIdentity.Path) -Right ([string]$Context.InstalledExecutable))) {
        throw "listener is not a CCSwitchMulti runtime from the installed path"
    }
    Assert-CcsmHealthyResponse -Health $Health -Label "existing CCSwitchMulti runtime"
    if ($ActualVersion -ne $ExpectedVersion) {
        throw "existing CCSwitchMulti runtime version mismatch: actual=$ActualVersion expected=$ExpectedVersion"
    }
    if (-not [string]::Equals($ActualHash, $ExpectedHash, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "existing CCSwitchMulti runtime hash mismatch: actual=$ActualHash expected=$ExpectedHash"
    }
    return [int]$ListenerIdentity.ProcessId
}

function Assert-CcsmVerifiedStopTarget {
    param($Context, [hashtable]$Operations)

    $currentIdentity = & $Operations.GetProcessIdentity $Context.CurrentPid
    if (-not (Test-CcsmSameProcessIdentity -Expected $Context.CurrentProcessIdentity -Actual $currentIdentity)) {
        throw "current process instance changed after preflight"
    }
    $listenerOwner = & $Operations.GetListenerOwner $Context.Port
    if ($listenerOwner -ne $Context.CurrentPid) {
        throw "listener owner changed after preflight"
    }
    return $currentIdentity
}

function Stop-CcsmVerifiedProcessHandle {
    param($Process, [int]$TimeoutSeconds)

    $timeoutMilliseconds = [int]([int64]$TimeoutSeconds * 1000)
    $Process.Kill()
    if (-not $Process.WaitForExit($timeoutMilliseconds)) {
        throw "verified process did not exit before timeout"
    }
}

function Write-CcsmBestEffortLog {
    param(
        [hashtable]$Operations,
        $Context,
        [string]$Level,
        [string]$Event,
        [hashtable]$Detail,
        [System.Collections.Generic.List[string]]$Errors
    )

    try {
        & $Operations.WriteLog $Context $Level $Event $Detail
    } catch {
        if ($null -ne $Errors) {
            $Errors.Add("log ${Event}: $($_.Exception.Message)") | Out-Null
        }
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
        $verifiedStopIdentity = Assert-CcsmVerifiedStopTarget -Context $context -Operations $Operations
        & $Operations.StopVerifiedProcess $context $verifiedStopIdentity
        & $Operations.WaitPortReleased $context
        & $Operations.RetireRunMarker $context $verifiedStopIdentity
        [void](& $Operations.SnapshotConfig $context $backup)
        & $Operations.VerifyConfigSnapshot $context $backup
        & $Operations.RunUninstaller $context
        & $Operations.RunInstaller $context
        $newPid = & $Operations.StartProcess $context "new"
        & $Operations.WaitReady $context $newPid
        Assert-CcsmRuntime -Context $context -Operations $Operations -ProcessId $newPid `
            -ExpectedVersion $context.ExpectedInstalledVersion -ExpectedHash $context.ExpectedInstalledHash -Label "new runtime"
        Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "info" -Event "transaction-success" `
            -Detail @{ NewPid = $newPid } -Errors $null
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
        $rollbackErrors = New-Object System.Collections.Generic.List[string]
        Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "error" -Event "transaction-failed" `
            -Detail @{ Error = $transactionError } -Errors $rollbackErrors
        if (-not $rollbackRequired) {
            throw
        }

        Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "warning" -Event "rollback-start" `
            -Detail @{ Error = $transactionError } -Errors $rollbackErrors
        if ($null -ne $newPid) {
            $newIdentity = $null
            try {
                $newIdentity = & $Operations.GetProcessIdentity $newPid
            } catch {
                Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "warning" `
                    -Event "rollback-new-process-not-verifiable" -Detail @{ ProcessId = $newPid; Error = $_.Exception.Message } `
                    -Errors $rollbackErrors
            }
            if ($null -ne $newIdentity -and [int]$newIdentity.ProcessId -eq [int]$newPid -and
                (Test-CcsmSamePath -Left ([string]$newIdentity.Path) -Right $context.InstalledExecutable) -and
                -not [string]::IsNullOrWhiteSpace([string]$newIdentity.StartTime)) {
                try {
                    & $Operations.StopVerifiedProcess $context $newIdentity
                } catch {
                    $rollbackErrors.Add("stop new process: $($_.Exception.Message)") | Out-Null
                }
            } elseif ($null -ne $newIdentity) {
                Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "warning" `
                    -Event "rollback-skip-unverified-process" -Detail @{ ProcessId = $newPid } -Errors $rollbackErrors
            }
        }

        $restoreValidated = $true
        try {
            & $Operations.ValidateRestoreBackup $context $backup
        } catch {
            $restoreValidated = $false
            $rollbackErrors.Add("validate restore backup: $($_.Exception.Message)") | Out-Null
        }
        if ($restoreValidated) {
            try {
                & $Operations.RestoreAppAndConfig $context $backup
            } catch {
                $rollbackErrors.Add("restore app and config: $($_.Exception.Message)") | Out-Null
            }
            try {
                & $Operations.DeleteRegistryKey $context $backup
            } catch {
                $rollbackErrors.Add("delete restored registry key: $($_.Exception.Message)") | Out-Null
            }
            try {
                & $Operations.ImportRegistry $context $backup
            } catch {
                $rollbackErrors.Add("import restored registry key: $($_.Exception.Message)") | Out-Null
            }
            try {
                & $Operations.VerifyRegistryRestore $context $backup
            } catch {
                $rollbackErrors.Add("verify restored registry key: $($_.Exception.Message)") | Out-Null
            }
            try {
                & $Operations.VerifyRestoredState $context $backup
            } catch {
                $rollbackErrors.Add("verify restored app and config: $($_.Exception.Message)") | Out-Null
            }
        } else {
            Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "warning" `
                -Event "rollback-restore-skipped-invalid-backup" -Detail @{ Error = $transactionError } -Errors $rollbackErrors
        }

        $previousPid = $null
        if ($restoreValidated) {
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
            } else {
                $rollbackErrors.Add("start previous runtime: no process ID returned") | Out-Null
            }
        } else {
            $rollbackErrors.Add("start previous runtime: skipped because backup validation failed") | Out-Null
        }

        if ($rollbackErrors.Count -eq 0) {
            Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "info" -Event "rollback-success" `
                -Detail @{ PreviousPid = $previousPid } -Errors $rollbackErrors
        }
        if ($rollbackErrors.Count -eq 0) {
            return [pscustomobject]@{ Status = "RolledBack"; TransactionId = $context.TransactionId; BackupPath = $backup.Path; NewPid = $newPid; Error = $transactionError; RollbackError = $null }
        }
        $rollbackError = $rollbackErrors -join "; "
        Write-CcsmBestEffortLog -Operations $Operations -Context $context -Level "error" -Event "rollback-failed" `
            -Detail @{ Error = $transactionError; RollbackError = $rollbackError } -Errors $rollbackErrors
        $rollbackError = $rollbackErrors -join "; "
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

    if (-not (Test-CcsmStrictDescendant -Candidate $Context.TransactionRoot -Parent $Context.BackupRoot) -or
        -not (Test-CcsmSamePath -Left $BackupPath -Right $Context.TransactionRoot)) {
        throw "backup path escaped the validated transaction boundary"
    }
    Assert-CcsmProductInstallBoundary -InstallDirectory $Context.InstallDirectory `
        -InstalledExecutable $Context.InstalledExecutable -UninstallExecutable $Context.UninstallExecutable
    Assert-CcsmProductConfigBoundary -ConfigPaths $Context.ConfigPaths
    Assert-CcsmRegistryKey -Key $Context.RegistryKey
    Assert-CcsmNoReparseBoundary -Path @(
        $Context.BackupRoot, $Context.TransactionRoot, $BackupPath, $Context.InstallDirectory
    ) -Purpose "transaction restore"
    foreach ($configPath in @($Context.ConfigPaths)) {
        Assert-CcsmAuthoritativeConfigBoundary -ConfigRoot $configPath -Purpose "transaction restore config"
    }
}

function Copy-CcsmDirectoryContents {
    param([string]$Source, [string]$Destination)

    Assert-CcsmNoReparseBoundary -Path @($Source, $Destination) -Purpose "directory copy"
    if (-not (Test-Path -LiteralPath $Source -PathType Container)) {
        throw "directory copy source is missing: $Source"
    }
    if (-not (Test-Path -LiteralPath $Destination -PathType Container)) {
        New-Item -ItemType Directory -Path $Destination -Force -ErrorAction Stop | Out-Null
    }
    foreach ($item in @(Get-ChildItem -LiteralPath $Source -Force -ErrorAction Stop)) {
        Copy-Item -LiteralPath $item.FullName -Destination $Destination -Recurse -Force -ErrorAction Stop
    }
}

function Get-CcsmAuthoritativeConfigFileNames {
    return @(
        "cc-switch.db",
        "cc-switch.db-wal",
        "cc-switch.db-shm",
        "settings.json",
        "model-pricing.json",
        "codex-desktop-executable.json",
        "codex_oauth_auth.json"
    )
}

function Assert-CcsmAuthoritativeConfigBoundary {
    param([string]$ConfigRoot, [string]$Purpose = "config boundary")

    Assert-CcsmNoReparsePathBoundary -Path $ConfigRoot -Purpose $Purpose
    foreach ($name in @(Get-CcsmAuthoritativeConfigFileNames)) {
        $candidate = Join-Path $ConfigRoot $name
        Assert-CcsmNoReparsePathBoundary -Path $candidate -Purpose $Purpose
        if (Test-Path -LiteralPath $candidate) {
            $item = Get-Item -LiteralPath $candidate -Force -ErrorAction Stop
            if ($item.PSIsContainer) {
                throw "authoritative config path is not a regular file: $candidate"
            }
        }
    }
}

function Copy-CcsmAuthoritativeConfigFiles {
    param(
        [string]$Source,
        [string]$Destination,
        [switch]$ReplaceDestination
    )

    if (-not (Test-Path -LiteralPath $Source -PathType Container)) {
        throw "config copy source is missing: $Source"
    }
    Assert-CcsmAuthoritativeConfigBoundary -ConfigRoot $Source -Purpose "config copy source"
    $databasePath = Join-Path $Source "cc-switch.db"
    if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf)) {
        throw "config database is missing: $databasePath"
    }
    Assert-CcsmAuthoritativeConfigBoundary -ConfigRoot $Destination -Purpose "config copy destination"
    if (-not (Test-Path -LiteralPath $Destination -PathType Container)) {
        New-Item -ItemType Directory -Path $Destination -Force -ErrorAction Stop | Out-Null
    }

    $names = @(Get-CcsmAuthoritativeConfigFileNames)
    if ($ReplaceDestination) {
        foreach ($name in $names) {
            $destinationPath = Join-Path $Destination $name
            if (Test-Path -LiteralPath $destinationPath) {
                $destinationItem = Get-Item -LiteralPath $destinationPath -Force -ErrorAction Stop
                if ($destinationItem.PSIsContainer) {
                    throw "authoritative config path is not a regular file: $destinationPath"
                }
                Remove-Item -LiteralPath $destinationPath -Force -ErrorAction Stop
            }
        }
    }
    foreach ($name in $names) {
        $sourcePath = Join-Path $Source $name
        if (Test-Path -LiteralPath $sourcePath -PathType Leaf) {
            Copy-Item -LiteralPath $sourcePath -Destination (Join-Path $Destination $name) -Force -ErrorAction Stop
        }
    }
}

function Get-CcsmRegularFileInventory {
    param([string]$Root)

    Assert-CcsmNoReparseBoundary -Path $Root -Purpose "app backup inventory"
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) {
        throw "app backup inventory"
    }
    $rootPath = [System.IO.Path]::GetFullPath($Root).TrimEnd([char[]]@([char]92, [char]47))
    $records = New-Object System.Collections.Generic.List[string]
    foreach ($file in @(Get-ChildItem -LiteralPath $Root -File -Force -Recurse -ErrorAction Stop)) {
        $filePath = [System.IO.Path]::GetFullPath($file.FullName)
        $separator = [System.IO.Path]::DirectorySeparatorChar
        if (-not $filePath.StartsWith("$rootPath$separator", [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "app backup inventory"
        }
        $relativePath = $filePath.Substring($rootPath.Length).TrimStart([char[]]@([char]92, [char]47))
        if ([string]::IsNullOrWhiteSpace($relativePath) -or $relativePath -match '(^|[\\/])\.\.([\\/]|$)') {
            throw "app backup inventory"
        }
        $hash = (Get-CcsmSha256 -LiteralPath $file.FullName).ToUpperInvariant()
        [void]$records.Add("$relativePath`t$hash")
    }
    $orderedRecords = [string[]]$records.ToArray()
    [Array]::Sort($orderedRecords, [System.StringComparer]::Ordinal)
    $files = New-Object System.Collections.Generic.List[object]
    foreach ($record in $orderedRecords) {
        $tabIndex = $record.IndexOf([char]9)
        if ($tabIndex -lt 1) { throw "app backup inventory" }
        [void]$files.Add([pscustomobject]@{
            RelativePath = $record.Substring(0, $tabIndex)
            Hash = $record.Substring($tabIndex + 1)
        })
    }
    $digestAlgorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digestBytes = $digestAlgorithm.ComputeHash([System.Text.Encoding]::UTF8.GetBytes([string]::Join("`n", $orderedRecords)))
        $digest = ([System.BitConverter]::ToString($digestBytes)).Replace("-", "")
    } finally {
        $digestAlgorithm.Dispose()
    }
    return [pscustomobject]@{ Files = $files.ToArray(); Digest = $digest }
}

function Assert-CcsmRegularFileInventory {
    param($Expected, [string]$Root)

    if ($null -eq $Expected) { throw "app backup inventory" }
    $filesProperty = $Expected.PSObject.Properties["Files"]
    $digestProperty = $Expected.PSObject.Properties["Digest"]
    if ($null -eq $filesProperty -or $null -eq $digestProperty) { throw "app backup inventory" }
    $expectedFiles = @($filesProperty.Value)
    $expectedDigest = [string]$digestProperty.Value
    if ($expectedDigest -notmatch '^[A-Fa-f0-9]{64}$') { throw "app backup inventory" }
    foreach ($expectedFile in $expectedFiles) {
        if ($null -eq $expectedFile -or [string]::IsNullOrWhiteSpace([string]$expectedFile.RelativePath) -or
            [string]$expectedFile.RelativePath -match '(^|[\\/])\.\.([\\/]|$)' -or
            [string]$expectedFile.Hash -notmatch '^[A-Fa-f0-9]{64}$') {
            throw "app backup inventory"
        }
    }
    $actual = Get-CcsmRegularFileInventory -Root $Root
    if (-not [string]::Equals($actual.Digest, $expectedDigest, [System.StringComparison]::OrdinalIgnoreCase) -or
        $actual.Files.Count -ne $expectedFiles.Count) {
        throw "app backup inventory"
    }
    foreach ($expectedFile in $expectedFiles) {
        $matches = @($actual.Files | Where-Object {
            $_.RelativePath -eq $expectedFile.RelativePath -and
            [string]::Equals($_.Hash, $expectedFile.Hash, [System.StringComparison]::OrdinalIgnoreCase)
        })
        if ($matches.Count -ne 1) { throw "app backup inventory" }
    }
}

function Get-CcsmConfigInventory {
    param([string]$ConfigRoot)

    Assert-CcsmAuthoritativeConfigBoundary -ConfigRoot $ConfigRoot -Purpose "config inventory"
    if (-not (Test-Path -LiteralPath $ConfigRoot -PathType Container)) {
        throw "config root is missing: $ConfigRoot"
    }
    $databasePath = Join-Path $ConfigRoot "cc-switch.db"
    if (-not (Test-Path -LiteralPath $databasePath -PathType Leaf)) {
        throw "config database is missing: $databasePath"
    }
    $files = @()
    foreach ($name in @(Get-CcsmAuthoritativeConfigFileNames)) {
        $filePath = Join-Path $ConfigRoot $name
        if (Test-Path -LiteralPath $filePath -PathType Leaf) {
            $files += [pscustomobject]@{
                RelativePath = $name
                Hash = Get-CcsmSha256 -LiteralPath $filePath
            }
        }
    }
    $sidecars = @()
    foreach ($name in @("cc-switch.db", "cc-switch.db-wal", "cc-switch.db-shm")) {
        $candidate = Join-Path $ConfigRoot $name
        $exists = Test-Path -LiteralPath $candidate -PathType Leaf
        $sidecars += [pscustomobject]@{
            Name = $name
            Exists = [bool]$exists
            Hash = if ($exists) { Get-CcsmSha256 -LiteralPath $candidate } else { $null }
        }
    }
    return [pscustomobject]@{ Files = $files; Sidecars = $sidecars }
}

function Invoke-CcsmSqliteIntegrityCheck {
    param([string]$DatabasePath)

    if (-not (Test-Path -LiteralPath $DatabasePath -PathType Leaf)) {
        throw "SQLite database is missing: $DatabasePath"
    }
    if ($null -eq ("CcsmSqliteNative" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CcsmSqliteNative {
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Unicode, ExactSpelling = true)]
    public static extern int sqlite3_open16(string filename, out IntPtr db);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Unicode, ExactSpelling = true)]
    public static extern int sqlite3_prepare16_v2(IntPtr db, string sql, int bytes, out IntPtr statement, IntPtr tail);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    public static extern int sqlite3_step(IntPtr statement);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl, CharSet = CharSet.Unicode, ExactSpelling = true)]
    public static extern IntPtr sqlite3_column_text16(IntPtr statement, int column);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    public static extern int sqlite3_finalize(IntPtr statement);
    [DllImport("winsqlite3.dll", CallingConvention = CallingConvention.Cdecl)]
    public static extern int sqlite3_close(IntPtr db);
}
'@ -ErrorAction Stop
    }
    $database = [IntPtr]::Zero
    $statement = [IntPtr]::Zero
    try {
        if ([CcsmSqliteNative]::sqlite3_open16($DatabasePath, [ref]$database) -ne 0) {
            throw "SQLite open failed for integrity check"
        }
        if ([CcsmSqliteNative]::sqlite3_prepare16_v2($database, "PRAGMA integrity_check", -1, [ref]$statement, [IntPtr]::Zero) -ne 0) {
            throw "SQLite integrity_check preparation failed"
        }
        if ([CcsmSqliteNative]::sqlite3_step($statement) -ne 100) {
            throw "SQLite integrity_check did not return a result"
        }
        $resultPointer = [CcsmSqliteNative]::sqlite3_column_text16($statement, 0)
        $result = [Runtime.InteropServices.Marshal]::PtrToStringUni($resultPointer)
        if ($result -ne "ok") {
            throw "SQLite integrity_check failed: $result"
        }
    } finally {
        if ($statement -ne [IntPtr]::Zero) { [void][CcsmSqliteNative]::sqlite3_finalize($statement) }
        if ($database -ne [IntPtr]::Zero) { [void][CcsmSqliteNative]::sqlite3_close($database) }
    }
}

function Assert-CcsmConfigInventory {
    param($Expected, [string]$ConfigRoot)

    $actual = Get-CcsmConfigInventory -ConfigRoot $ConfigRoot
    $expectedFiles = @($Expected.Files)
    $actualFiles = @($actual.Files)
    if ($expectedFiles.Count -ne $actualFiles.Count) {
        throw "config file inventory count changed"
    }
    foreach ($expectedFile in $expectedFiles) {
        $match = @($actualFiles | Where-Object {
            $_.RelativePath -eq $expectedFile.RelativePath -and
            [string]::Equals($_.Hash, $expectedFile.Hash, [System.StringComparison]::OrdinalIgnoreCase)
        })
        if ($match.Count -ne 1) { throw "config file hash changed: $($expectedFile.RelativePath)" }
    }
    foreach ($expectedSidecar in @($Expected.Sidecars)) {
        $match = @($actual.Sidecars | Where-Object {
            $_.Name -eq $expectedSidecar.Name -and $_.Exists -eq $expectedSidecar.Exists -and
            [string]::Equals([string]$_.Hash, [string]$expectedSidecar.Hash, [System.StringComparison]::OrdinalIgnoreCase)
        })
        if ($match.Count -ne 1) { throw "config SQLite sidecar changed: $($expectedSidecar.Name)" }
    }
    Invoke-CcsmSqliteIntegrityCheck -DatabasePath (Join-Path $ConfigRoot "cc-switch.db")
}

function Write-CcsmBackupManifest {
    param($Context, $Backup)

    $manifest = [ordered]@{
        TransactionId = $Context.TransactionId
        InstallDirectory = $Context.InstallDirectory
        AppBackup = $Backup.AppBackup
        AppInventory = @($Backup.AppInventory)
        AppInventoryDigest = $Backup.AppInventoryDigest
        ConfigSnapshotComplete = [bool]$Backup.ConfigSnapshotComplete
        ConfigBackups = @($Backup.ConfigBackups)
        RegistryKey = $Context.RegistryKey
        RegistryExisted = [bool]$Backup.RegistryExisted
        RegistryFile = $Backup.RegistryFile
        RegistryFileHash = $Backup.RegistryFileHash
    }
    $Backup.ManifestPath = Join-Path $Backup.Path "backup-manifest.json"
    $manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $Backup.ManifestPath -Encoding UTF8 -ErrorAction Stop
    $Backup.ManifestHash = Get-CcsmSha256 -LiteralPath $Backup.ManifestPath
}

function Get-CcsmValidatedBackupManifest {
    param($Context, $Backup)

    Assert-CcsmRestoreBoundary -Context $Context -BackupPath ([string]$Backup.Path)
    if (-not (Test-CcsmSamePath -Left ([string]$Backup.ManifestPath) -Right (Join-Path $Context.TransactionRoot "backup-manifest.json")) -or
        -not (Test-Path -LiteralPath $Backup.ManifestPath -PathType Leaf)) {
        throw "backup manifest is missing or escaped the transaction boundary"
    }
    Assert-CcsmNoReparseBoundary -Path $Backup.ManifestPath -Purpose "backup manifest"
    Assert-CcsmHash -Name "backup manifest hash" -Value ([string]$Backup.ManifestHash)
    $actualManifestHash = Get-CcsmSha256 -LiteralPath $Backup.ManifestPath
    if (-not [string]::Equals($actualManifestHash, $Backup.ManifestHash, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "backup manifest integrity hash mismatch"
    }
    $manifest = Get-Content -LiteralPath $Backup.ManifestPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    if ($manifest.TransactionId -ne $Context.TransactionId -or
        -not (Test-CcsmSamePath -Left ([string]$manifest.InstallDirectory) -Right $Context.InstallDirectory) -or
        -not (Test-CcsmStrictDescendant -Candidate ([string]$manifest.AppBackup) -Parent $Context.TransactionRoot) -or
        -not (Test-Path -LiteralPath $manifest.AppBackup -PathType Container)) {
        throw "backup manifest does not match the validated transaction"
    }
    Assert-CcsmNoReparseBoundary -Path ([string]$manifest.AppBackup) -Purpose "app backup manifest source"
    $appInventoryProperty = $manifest.PSObject.Properties["AppInventory"]
    $appInventoryDigestProperty = $manifest.PSObject.Properties["AppInventoryDigest"]
    if ($null -eq $appInventoryProperty -or $null -eq $appInventoryDigestProperty) {
        throw "app backup inventory"
    }
    $appInventory = [pscustomobject]@{
        Files = @($appInventoryProperty.Value)
        Digest = [string]$appInventoryDigestProperty.Value
    }
    Assert-CcsmRegularFileInventory -Expected $appInventory -Root ([string]$manifest.AppBackup)
    Assert-CcsmRegistryKey -Key ([string]$manifest.RegistryKey)
    $configBackups = @($manifest.ConfigBackups)
    if ([bool]$manifest.ConfigSnapshotComplete -and $configBackups.Count -ne $Context.ConfigPaths.Count) {
        throw "config snapshot manifest count mismatch"
    }
    if (-not [bool]$manifest.ConfigSnapshotComplete -and $configBackups.Count -ne 0) {
        throw "incomplete config snapshot has backup entries"
    }
    foreach ($configBackup in $configBackups) {
        $expectedConfig = @($Context.ConfigPaths | Where-Object { Test-CcsmSamePath -Left $_ -Right ([string]$configBackup.Source) })
        if ($expectedConfig.Count -ne 1 -or
            -not (Test-CcsmStrictDescendant -Candidate ([string]$configBackup.Backup) -Parent $Context.TransactionRoot)) {
            throw "config backup escaped the validated restore boundary"
        }
        Assert-CcsmAuthoritativeConfigBoundary -ConfigRoot ([string]$configBackup.Source) -Purpose "config backup manifest source"
        Assert-CcsmAuthoritativeConfigBoundary -ConfigRoot ([string]$configBackup.Backup) -Purpose "config backup manifest source"
        if (-not (Test-Path -LiteralPath $configBackup.Backup -PathType Container)) {
            throw "config backup escaped the validated restore boundary"
        }
    }
    if ([bool]$manifest.RegistryExisted) {
        if (-not (Test-CcsmStrictDescendant -Candidate ([string]$manifest.RegistryFile) -Parent $Context.TransactionRoot)) {
            throw "registry backup escaped the validated restore boundary"
        }
        Assert-CcsmNoReparseBoundary -Path ([string]$manifest.RegistryFile) -Purpose "registry backup manifest source"
        if (-not (Test-Path -LiteralPath $manifest.RegistryFile -PathType Leaf)) {
            throw "registry backup escaped the validated restore boundary"
        }
        Assert-CcsmHash -Name "registry backup hash" -Value ([string]$manifest.RegistryFileHash)
        $actualRegistryHash = Get-CcsmSha256 -LiteralPath $manifest.RegistryFile
        if (-not [string]::Equals($actualRegistryHash, $manifest.RegistryFileHash, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "registry backup integrity hash mismatch"
        }
    }
    return $manifest
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
        if (@("install-directory", "backup-root", "config") -contains $Kind) {
            return $item.PSIsContainer
        }
        return $true
    }
    $operations.GetFileHash = {
        param($Path)
        return Get-CcsmSha256 -LiteralPath $Path
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
    $operations.GetProcessIdentity = {
        param($ProcessId)
        $process = Get-Process -Id $ProcessId -ErrorAction Stop
        return [pscustomobject]@{
            ProcessId = [int]$process.Id
            Path = $process.MainModule.FileName
            StartTime = $process.StartTime.ToUniversalTime().ToString("o")
            Handle = $process
        }
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
        New-Item -ItemType Directory -Path $appBackup -Force -ErrorAction Stop | Out-Null
        Copy-CcsmDirectoryContents -Source $Context.InstallDirectory -Destination $appBackup
        $appInventory = Get-CcsmRegularFileInventory -Root $appBackup

        $registryExisted = Test-Path -LiteralPath $Context.RegistryKey
        $registryFile = $null
        $registryFileHash = $null
        if ($registryExisted) {
            $registryFile = Join-Path $Context.TransactionRoot "registry.reg"
            $nativeKey = ConvertTo-CcsmNativeRegistryPath $Context.RegistryKey
            & reg.exe export $nativeKey $registryFile /y | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "registry export failed with exit code $LASTEXITCODE" }
            $registryFileHash = Get-CcsmSha256 -LiteralPath $registryFile
        }
        $backup = [pscustomobject]@{
            Path = $Context.TransactionRoot
            AppBackup = $appBackup
            AppInventory = @($appInventory.Files)
            AppInventoryDigest = $appInventory.Digest
            ConfigSnapshotComplete = $false
            ConfigBackups = @()
            RegistryExisted = [bool]$registryExisted
            RegistryFile = $registryFile
            RegistryFileHash = $registryFileHash
            ManifestPath = $null
            ManifestHash = $null
        }
        Write-CcsmBackupManifest -Context $Context -Backup $backup
        return $backup
    }
    $operations.SnapshotConfig = {
        param($Context, $Backup)
        Assert-CcsmRestoreBoundary -Context $Context -BackupPath $Backup.Path
        $configRoot = Join-Path $Backup.Path "config"
        New-Item -ItemType Directory -Path $configRoot -Force -ErrorAction Stop | Out-Null
        $configBackups = @()
        for ($index = 0; $index -lt $Context.ConfigPaths.Count; $index++) {
            $source = $Context.ConfigPaths[$index]
            $destination = Join-Path $configRoot ([string]$index)
            Invoke-CcsmSqliteIntegrityCheck -DatabasePath (Join-Path $source "cc-switch.db")
            New-Item -ItemType Directory -Path $destination -Force -ErrorAction Stop | Out-Null
            Copy-CcsmAuthoritativeConfigFiles -Source $source -Destination $destination -ReplaceDestination
            $inventory = Get-CcsmConfigInventory -ConfigRoot $destination
            Invoke-CcsmSqliteIntegrityCheck -DatabasePath (Join-Path $destination "cc-switch.db")
            $configBackups += [pscustomobject]@{
                Source = $source
                Backup = $destination
                Files = @($inventory.Files)
                Sidecars = @($inventory.Sidecars)
            }
        }
        $Backup.ConfigBackups = $configBackups
        $Backup.ConfigSnapshotComplete = $true
        Write-CcsmBackupManifest -Context $Context -Backup $Backup
    }
    $operations.VerifyConfigSnapshot = {
        param($Context, $Backup)
        $manifest = Get-CcsmValidatedBackupManifest -Context $Context -Backup $Backup
        if (-not [bool]$manifest.ConfigSnapshotComplete) { throw "config snapshot was not completed" }
        foreach ($configBackup in @($manifest.ConfigBackups)) {
            Assert-CcsmConfigInventory -Expected $configBackup -ConfigRoot ([string]$configBackup.Backup)
        }
    }
    $operations.StopVerifiedProcess = {
        param($Context, $ExpectedIdentity)
        $liveIdentity = & $operations.GetProcessIdentity ([int]$ExpectedIdentity.ProcessId)
        if (-not (Test-CcsmSameProcessIdentity -Expected $ExpectedIdentity -Actual $liveIdentity)) {
            throw "process instance changed before verified stop"
        }
        if ($null -eq $liveIdentity.Handle) { throw "verified process handle is unavailable" }
        Stop-CcsmVerifiedProcessHandle -Process $liveIdentity.Handle -TimeoutSeconds $Context.TimeoutSeconds
    }
    $operations.WaitPortReleased = {
        param($Context)
        $state = [pscustomobject]@{ ReplacementStops = 0 }
        Wait-CcsmCondition -TimeoutSeconds $Context.TimeoutSeconds -Description "port $($Context.Port) release" -Condition {
            $owner = & $operations.GetListenerOwner $Context.Port
            if ($null -eq $owner) { return $true }
            try {
                $identity = & $operations.GetProcessIdentity ([int]$owner)
            } catch {
                # The TCP row can briefly outlive the process handle. Poll again instead of treating
                # the stale owner as a product replacement or a foreign listener.
                return $false
            }
            $action = Resolve-CcsmReplacementListenerAction -Context $Context -ListenerIdentity $identity
            if ($action -eq "stop") {
                $state.ReplacementStops++
                if ($state.ReplacementStops -gt 3) {
                    throw "CCSwitchMulti repeatedly reclaimed port $($Context.Port) during install"
                }
                & $operations.WriteLog $Context "warning" "replacement-listener-stopped" @{
                    ProcessId = [int]$identity.ProcessId
                    Path = [string]$identity.Path
                    Attempt = [int]$state.ReplacementStops
                }
                & $operations.StopVerifiedProcess $Context $identity
            }
            return $false
        }
    }
    $operations.RetireRunMarker = {
        param($Context, $ExpectedIdentity)
        foreach ($configPath in $Context.ConfigPaths) {
            Remove-CcsmVerifiedRunMarker -ConfigPath $configPath -ExpectedIdentity $ExpectedIdentity `
                -ExpectedVersion $Context.ExpectedCurrentVersion
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
        $owner = & $operations.GetListenerOwner $Context.Port
        if ($null -ne $owner) {
            $identity = & $operations.GetProcessIdentity ([int]$owner)
            $expectedVersion = if ($Mode -eq "new") { $Context.ExpectedInstalledVersion } else { $Context.ExpectedCurrentVersion }
            $expectedHash = if ($Mode -eq "new") { $Context.ExpectedInstalledHash } else { $Context.ExpectedCurrentHash }
            $actualVersion = & $operations.GetFileVersion $Context.InstalledExecutable
            $actualHash = & $operations.GetFileHash $Context.InstalledExecutable
            $health = & $operations.GetHealth $Context.HealthUri
            $adoptedPid = Resolve-CcsmExistingRuntimeProcessId -Context $Context -ListenerIdentity $identity `
                -ExpectedVersion $expectedVersion -ExpectedHash $expectedHash `
                -ActualVersion $actualVersion -ActualHash $actualHash -Health $health
            & $operations.WriteLog $Context "warning" "existing-listener-adopted" @{
                ProcessId = $adoptedPid
                Mode = $Mode
            }
            return $adoptedPid
        }
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
    $operations.ValidateRestoreBackup = {
        param($Context, $Backup)
        [void](Get-CcsmValidatedBackupManifest -Context $Context -Backup $Backup)
    }
    $operations.RestoreAppAndConfig = {
        param($Context, $Backup)
        $manifest = Get-CcsmValidatedBackupManifest -Context $Context -Backup $Backup

        Remove-CcsmDirectoryTree -Path $Context.InstallDirectory -Purpose "restore install directory"
        New-Item -ItemType Directory -Path $Context.InstallDirectory -Force -ErrorAction Stop | Out-Null
        Copy-CcsmDirectoryContents -Source ([string]$manifest.AppBackup) -Destination $Context.InstallDirectory

        if ([bool]$manifest.ConfigSnapshotComplete) {
            foreach ($configBackup in @($manifest.ConfigBackups)) {
                $source = [string]$configBackup.Source
                Copy-CcsmAuthoritativeConfigFiles -Source ([string]$configBackup.Backup) -Destination $source -ReplaceDestination
            }
        }
    }
    $operations.DeleteRegistryKey = {
        param($Context, $Backup)
        [void](Get-CcsmValidatedBackupManifest -Context $Context -Backup $Backup)
        Assert-CcsmRegistryKey -Key $Context.RegistryKey
        if (Test-Path -LiteralPath $Context.RegistryKey) {
            Remove-Item -LiteralPath $Context.RegistryKey -Recurse -Force -ErrorAction Stop
        }
    }
    $operations.ImportRegistry = {
        param($Context, $Backup)
        $manifest = Get-CcsmValidatedBackupManifest -Context $Context -Backup $Backup
        if ([bool]$manifest.RegistryExisted) {
            & reg.exe import ([string]$manifest.RegistryFile) | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "registry import failed with exit code $LASTEXITCODE" }
        }
    }
    $operations.VerifyRegistryRestore = {
        param($Context, $Backup)
        $manifest = Get-CcsmValidatedBackupManifest -Context $Context -Backup $Backup
        if (-not [bool]$manifest.RegistryExisted) {
            if (Test-Path -LiteralPath $Context.RegistryKey) { throw "registry key unexpectedly exists after restore" }
            return
        }
        if (-not (Test-Path -LiteralPath $Context.RegistryKey)) { throw "registry key is missing after import" }
        $verificationFile = Join-Path $Backup.Path "registry-verify.reg"
        Assert-CcsmRestoreBoundary -Context $Context -BackupPath $Backup.Path
        try {
            & reg.exe export (ConvertTo-CcsmNativeRegistryPath $Context.RegistryKey) $verificationFile /y | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "registry verification export failed with exit code $LASTEXITCODE" }
            $verificationHash = Get-CcsmSha256 -LiteralPath $verificationFile
            if (-not [string]::Equals($verificationHash, [string]$manifest.RegistryFileHash, [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "registry verification hash mismatch"
            }
        } finally {
            if (Test-Path -LiteralPath $verificationFile) {
                Remove-Item -LiteralPath $verificationFile -Force -ErrorAction SilentlyContinue
            }
        }
    }
    $operations.VerifyRestoredState = {
        param($Context, $Backup)
        $manifest = Get-CcsmValidatedBackupManifest -Context $Context -Backup $Backup
        $appInventory = [pscustomobject]@{
            Files = @($manifest.AppInventory)
            Digest = [string]$manifest.AppInventoryDigest
        }
        Assert-CcsmRegularFileInventory -Expected $appInventory -Root $Context.InstallDirectory
        $restoredVersion = & $operations.GetFileVersion $Context.InstalledExecutable
        if ($restoredVersion -ne $Context.ExpectedCurrentVersion) { throw "restored application version mismatch" }
        $restoredHash = & $operations.GetFileHash $Context.InstalledExecutable
        if (-not [string]::Equals($restoredHash, $Context.ExpectedCurrentHash, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "restored application hash mismatch"
        }
        if ([bool]$manifest.ConfigSnapshotComplete) {
            foreach ($configBackup in @($manifest.ConfigBackups)) {
                Assert-CcsmConfigInventory -Expected $configBackup -ConfigRoot ([string]$configBackup.Source)
            }
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
    exit (Get-CcsmTransactionExitCode -Result $result)
}
