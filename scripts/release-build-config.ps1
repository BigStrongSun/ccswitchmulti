function New-TauriBuildConfigFile {
    $path = [System.IO.Path]::GetTempFileName()
    try {
        $json = '{"bundle":{"createUpdaterArtifacts":false}}'
        $encoding = [System.Text.UTF8Encoding]::new($false)
        [System.IO.File]::WriteAllText($path, $json, $encoding)
        return $path
    } catch {
        [System.IO.File]::Delete($path)
        throw
    }
}

function Remove-TauriBuildConfigFile {
    param([string]$Path)

    if (-not [string]::IsNullOrWhiteSpace($Path)) {
        [System.IO.File]::Delete($Path)
    }
}

function Get-LocalReleaseCargoTargetRoot {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)

    $repoFull = [System.IO.Path]::GetFullPath($RepoRoot)
    return [System.IO.Path]::GetFullPath((Join-Path $repoFull ".tmp\local-release-cargo"))
}

function New-LocalReleaseCargoTargetDir {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)

    $cacheRoot = Get-LocalReleaseCargoTargetRoot -RepoRoot $RepoRoot
    [System.IO.Directory]::CreateDirectory($cacheRoot) | Out-Null
    $targetDir = Join-Path $cacheRoot ("run-$PID-" + [guid]::NewGuid().ToString("N"))
    [System.IO.Directory]::CreateDirectory($targetDir) | Out-Null
    return [System.IO.Path]::GetFullPath($targetDir)
}

function Remove-LocalReleaseCargoTargetDir {
    param(
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][string]$TargetDir
    )

    $cacheRoot = Get-LocalReleaseCargoTargetRoot -RepoRoot $RepoRoot
    $targetFull = [System.IO.Path]::GetFullPath($TargetDir)
    $targetParent = [System.IO.Path]::GetFullPath((Split-Path -Parent $targetFull))
    if (-not [string]::Equals(
            $targetParent,
            $cacheRoot,
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "local release Cargo target is outside the repository-owned local release Cargo cache root: $targetFull"
    }

    if (-not (Test-Path -LiteralPath $targetFull)) {
        return
    }
    $targetItem = Get-Item -LiteralPath $targetFull -Force
    if (-not $targetItem.PSIsContainer) {
        throw "local release Cargo target is not a directory: $targetFull"
    }
    if (($targetItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "local release Cargo target must not be a reparse point: $targetFull"
    }

    $manifestPath = Join-Path ([System.IO.Path]::GetFullPath($RepoRoot)) "src-tauri\Cargo.toml"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "Cargo manifest is missing for local release cleanup: $manifestPath"
    }
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        # Windows PowerShell 5.1 turns native stderr into error records. Cargo
        # writes successful cleanup progress there, so inspect its exit code.
        $ErrorActionPreference = "Continue"
        & cargo clean --manifest-path $manifestPath --target-dir $targetFull 2>&1 |
            ForEach-Object { Write-Host ([string]$_) }
        $cargoExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    if ($cargoExitCode -ne 0) {
        throw "cargo clean failed with exit code $cargoExitCode for local release target: $targetFull"
    }
}

function Resolve-CargoTargetDir {
    param(
        [Parameter(Mandatory = $true)][string]$TauriDir,
        [string]$RequestedTargetDir = ""
    )

    $tauriFull = [System.IO.Path]::GetFullPath($TauriDir)
    if ([string]::IsNullOrWhiteSpace($RequestedTargetDir)) {
        return [System.IO.Path]::GetFullPath((Join-Path $tauriFull "target"))
    }
    if ([System.IO.Path]::IsPathRooted($RequestedTargetDir)) {
        return [System.IO.Path]::GetFullPath($RequestedTargetDir)
    }

    $repoRoot = Split-Path -Parent $tauriFull
    return [System.IO.Path]::GetFullPath((Join-Path $repoRoot $RequestedTargetDir))
}

function Invoke-WithLocalReleaseCargoTarget {
    param(
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][bool]$Enabled,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )

    if (-not $Enabled) {
        return & $Action $null
    }

    $hadPreviousTarget = Test-Path Env:CARGO_TARGET_DIR
    $previousTarget = $env:CARGO_TARGET_DIR
    $targetDir = New-LocalReleaseCargoTargetDir -RepoRoot $RepoRoot
    try {
        $env:CARGO_TARGET_DIR = $targetDir
        return & $Action $targetDir
    } finally {
        if ($hadPreviousTarget) {
            $env:CARGO_TARGET_DIR = $previousTarget
        } else {
            Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
        }
        try {
            Remove-LocalReleaseCargoTargetDir -RepoRoot $RepoRoot -TargetDir $targetDir
        } catch {
            Write-Warning "local release Cargo target cleanup failed for '$targetDir': $($_.Exception.Message)"
        }
    }
}

function Get-ReleaseFileSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        $stream = [System.IO.File]::OpenRead($Path)
        try {
            $bytes = $sha256.ComputeHash($stream)
            return [System.BitConverter]::ToString($bytes).Replace("-", "")
        } finally {
            $stream.Dispose()
        }
    } finally {
        $sha256.Dispose()
    }
}

function Resolve-CcswitchmultiReleaseRoot {
    param(
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [string]$RequestedRoot = "",
        [string]$GitCommonDir
    )

    if (-not [string]::IsNullOrWhiteSpace($RequestedRoot)) {
        return $RequestedRoot
    }

    $resolvedCommonDir = $GitCommonDir
    if (-not $PSBoundParameters.ContainsKey("GitCommonDir")) {
        $commonDirOutput = @(
            & git -C $RepoRoot rev-parse --path-format=absolute --git-common-dir 2>$null
        )
        if ($LASTEXITCODE -eq 0) {
            $resolvedCommonDir = ($commonDirOutput | ForEach-Object { [string]$_ }) -join ""
        }
    }

    if ([string]::IsNullOrWhiteSpace($resolvedCommonDir)) {
        throw "cannot resolve the CCSwitchMulti main repository from Git common-dir metadata"
    }

    if (-not [System.IO.Path]::IsPathRooted($resolvedCommonDir)) {
        $resolvedCommonDir = [System.IO.Path]::GetFullPath((Join-Path $RepoRoot $resolvedCommonDir))
    } else {
        $resolvedCommonDir = [System.IO.Path]::GetFullPath($resolvedCommonDir)
    }

    if (-not [string]::Equals(
            [System.IO.Path]::GetFileName($resolvedCommonDir.TrimEnd('\', '/')),
            ".git",
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
        throw "cannot resolve the CCSwitchMulti main repository: Git common-dir is not a .git directory"
    }

    $mainRepositoryRoot = Split-Path -Parent $resolvedCommonDir
    $workspaceRoot = Split-Path -Parent $mainRepositoryRoot
    if ([string]::IsNullOrWhiteSpace($workspaceRoot)) {
        throw "cannot resolve the CCSwitchMulti workspace root from Git common-dir metadata"
    }

    $folderName = @([char]0x6700, [char]0x65B0, [char]0x7248, "ccswitchmulti") -join ""
    return Join-Path $workspaceRoot $folderName
}

function Assert-LocalTauriCliVersion {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)

    $packageJsonPath = Join-Path $RepoRoot "package.json"
    $installedPackageJsonPath = Join-Path $RepoRoot "node_modules\@tauri-apps\cli\package.json"
    if (-not (Test-Path -LiteralPath $packageJsonPath -PathType Leaf)) {
        throw "package.json is missing while validating the local Tauri CLI: $packageJsonPath"
    }
    if (-not (Test-Path -LiteralPath $installedPackageJsonPath -PathType Leaf)) {
        throw "local Tauri CLI package is not installed; run pnpm install --frozen-lockfile"
    }

    $packageJson = [System.IO.File]::ReadAllText($packageJsonPath) | ConvertFrom-Json -ErrorAction Stop
    $installedPackageJson = [System.IO.File]::ReadAllText($installedPackageJsonPath) | ConvertFrom-Json -ErrorAction Stop
    $expectedVersion = [string]$packageJson.devDependencies.'@tauri-apps/cli'
    $installedVersion = [string]$installedPackageJson.version
    if ($expectedVersion -notmatch '^\d+\.\d+\.\d+$') {
        throw "@tauri-apps/cli must be pinned to an exact version for local release builds: $expectedVersion"
    }
    if (-not [string]::Equals($installedVersion, $expectedVersion, [System.StringComparison]::Ordinal)) {
        throw "installed Tauri CLI package version mismatch: expected=$expectedVersion actual=$installedVersion; run pnpm install --frozen-lockfile"
    }

    Push-Location $RepoRoot
    try {
        $versionOutput = @(& pnpm exec tauri --version 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw "local Tauri CLI version command failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
    $reportedText = ($versionOutput | ForEach-Object { [string]$_ }) -join "`n"
    $reportedMatch = [regex]::Match($reportedText, '(?m)^tauri-cli (?<version>\d+\.\d+\.\d+)\s*$')
    if (-not $reportedMatch.Success) {
        throw "local Tauri CLI returned an unrecognized version: $reportedText"
    }
    $reportedVersion = $reportedMatch.Groups['version'].Value
    if (-not [string]::Equals($reportedVersion, $expectedVersion, [System.StringComparison]::Ordinal)) {
        throw "local Tauri CLI binary version mismatch: expected=$expectedVersion reported=$reportedVersion"
    }
}

function Get-ReleaseSourceIdentity {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)

    $commit = ((& git -C $RepoRoot rev-parse HEAD 2>$null) | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($commit)) {
        throw "cannot resolve release source commit from $RepoRoot"
    }

    $branch = ((& git -C $RepoRoot rev-parse --abbrev-ref HEAD 2>$null) | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($branch)) {
        throw "cannot resolve release source branch from $RepoRoot"
    }

    $status = ((& git -C $RepoRoot status --porcelain=v1 --untracked-files=no 2>$null) | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw "cannot inspect tracked release source state in $RepoRoot"
    }

    $packageJsonPath = Join-Path $RepoRoot "package.json"
    if (-not (Test-Path -LiteralPath $packageJsonPath -PathType Leaf)) {
        throw "package.json is missing while resolving release source identity: $packageJsonPath"
    }
    $packageJson = [System.IO.File]::ReadAllText($packageJsonPath) | ConvertFrom-Json -ErrorAction Stop
    $version = [string]$packageJson.version
    if ([string]::IsNullOrWhiteSpace($version)) {
        throw "package.json version is empty while resolving release source identity"
    }

    [pscustomobject]@{
        Commit = $commit
        Branch = $branch
        Version = $version
        TrackedWorktree = if ([string]::IsNullOrWhiteSpace($status)) { "clean" } else { "dirty" }
    }
}

function Assert-ReleaseSourceIdentity {
    param(
        [Parameter(Mandatory = $true)][psobject]$Expected,
        [Parameter(Mandatory = $true)][psobject]$Actual
    )

    $differences = @(
        "Commit",
        "Branch",
        "Version",
        "TrackedWorktree"
    ) | Where-Object {
        [string]$Expected.$_ -ne [string]$Actual.$_
    }
    if ($differences.Count -gt 0) {
        $details = $differences | ForEach-Object {
            "$_='$($Expected.$_)' -> '$($Actual.$_)'"
        }
        throw "release source identity changed: $($details -join '; ')"
    }
}

function New-ReleaseStageRoot {
    param([Parameter(Mandatory = $true)][string]$ReleaseRoot)

    $releaseFull = [System.IO.Path]::GetFullPath($ReleaseRoot)
    $parent = Split-Path -Parent $releaseFull
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    return Join-Path $parent (".ccswitchmulti-release-staging-$PID-$([guid]::NewGuid().ToString('N'))")
}

function Assert-ReleaseStagePair {
    param(
        [Parameter(Mandatory = $true)][string]$StageRoot,
        [Parameter(Mandatory = $true)][string]$ReleaseRoot
    )

    $stageFull = [System.IO.Path]::GetFullPath($StageRoot)
    $releaseFull = [System.IO.Path]::GetFullPath($ReleaseRoot)
    $stageParent = [System.IO.Path]::GetFullPath((Split-Path -Parent $stageFull))
    $releaseParent = [System.IO.Path]::GetFullPath((Split-Path -Parent $releaseFull))
    if (-not [string]::Equals($stageParent, $releaseParent, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "release staging path must be a sibling of the final release root"
    }
    if ([string]::Equals($stageFull, $releaseFull, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "release staging path must differ from the final release root"
    }
}

function Replace-ReleaseRootFromStage {
    param(
        [Parameter(Mandatory = $true)][string]$StageRoot,
        [Parameter(Mandatory = $true)][string]$ReleaseRoot
    )

    Assert-ReleaseStagePair -StageRoot $StageRoot -ReleaseRoot $ReleaseRoot
    $stageFull = [System.IO.Path]::GetFullPath($StageRoot)
    $releaseFull = [System.IO.Path]::GetFullPath($ReleaseRoot)
    if (-not (Test-Path -LiteralPath $stageFull -PathType Container)) {
        throw "release staging root is missing: $stageFull"
    }

    $backupFull = "$releaseFull.previous-$PID-$([guid]::NewGuid().ToString('N'))"
    $movedPrevious = $false
    try {
        if (Test-Path -LiteralPath $releaseFull) {
            Move-Item -LiteralPath $releaseFull -Destination $backupFull -ErrorAction Stop
            $movedPrevious = $true
        }
        Move-Item -LiteralPath $stageFull -Destination $releaseFull -ErrorAction Stop
    } catch {
        if ($movedPrevious -and -not (Test-Path -LiteralPath $releaseFull) -and (Test-Path -LiteralPath $backupFull)) {
            Move-Item -LiteralPath $backupFull -Destination $releaseFull -ErrorAction SilentlyContinue
        }
        throw
    }

    if (Test-Path -LiteralPath $backupFull) {
        try {
            Remove-Item -LiteralPath $backupFull -Recurse -Force -ErrorAction Stop
        } catch {
            Write-Warning "release backup could not be removed: $backupFull. The new release root is active."
        }
    }
}

function Get-TauriNsisInstalledExeSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    $unknownMarker = "__TAURI_BUNDLE_TYPE_VAR_UNK"
    $nsisMarker = "__TAURI_BUNDLE_TYPE_VAR_NSS"
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $latin1 = [System.Text.Encoding]::GetEncoding(28591)
    $binaryText = $latin1.GetString($bytes)
    $markerOffset = $binaryText.IndexOf(
        $unknownMarker,
        [System.StringComparison]::Ordinal
    )
    if ($markerOffset -lt 0 -or $binaryText.IndexOf(
            $unknownMarker,
            $markerOffset + 1,
            [System.StringComparison]::Ordinal
        ) -ge 0) {
        throw "raw Tauri executable must contain exactly one restored UNK bundle marker"
    }

    $replacement = [System.Text.Encoding]::ASCII.GetBytes($nsisMarker)
    [System.Array]::Copy($replacement, 0, $bytes, $markerOffset, $replacement.Length)

    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    try {
        return [System.BitConverter]::ToString($sha256.ComputeHash($bytes)).Replace("-", "")
    } finally {
        $sha256.Dispose()
    }
}
