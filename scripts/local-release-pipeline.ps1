param(
    [string]$ReleaseRoot = "",
    [switch]$SkipBuild,
    [switch]$NoTypecheck,
    [string]$Reason = "manual"
)

$ErrorActionPreference = "Stop"

$releaseBuildConfigHelperPath = Join-Path (Split-Path -Parent $PSCommandPath) "release-build-config.ps1"
. $releaseBuildConfigHelperPath

# Resolve the repository root for hook, terminal, and scheduled calls.
function Get-RepoRoot {
    $scriptDir = Split-Path -Parent $PSCommandPath
    return (Resolve-Path (Join-Path $scriptDir "..")).Path
}

# Write timestamped log lines so post-commit background failures are traceable.
function Write-Log {
    param([string]$Message)

    $line = "[{0}] {1}" -f (Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $Message
    Write-Host $line
}

# Run a command and stop the pipeline with a clear error when it fails.
function Invoke-CheckedCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )

    Write-Log ("RUN {0} {1}" -f $FilePath, ($Arguments -join " "))
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$FilePath failed with exit code $LASTEXITCODE"
    }
}

# Create a lock file so repeated commits cannot run multiple Tauri builds at the same time.
function Enter-PipelineLock {
    param([string]$LockPath)

    if (Test-Path -LiteralPath $LockPath) {
        $lockAge = (Get-Date) - (Get-Item -LiteralPath $LockPath).LastWriteTime
        if ($lockAge.TotalHours -lt 6) {
            throw "local release pipeline is already running. Lock: $LockPath"
        }
        Remove-Item -LiteralPath $LockPath -Force
    }

    $token = "$PID-$([guid]::NewGuid().ToString('N'))"
    try {
        $stream = [System.IO.File]::Open(
            $LockPath,
            [System.IO.FileMode]::CreateNew,
            [System.IO.FileAccess]::Write,
            [System.IO.FileShare]::None
        )
        try {
            $content = "$token`r`n$((Get-Date).ToString('o'))"
            $bytes = [System.Text.Encoding]::UTF8.GetBytes($content)
            $stream.Write($bytes, 0, $bytes.Length)
        } finally {
            $stream.Dispose()
        }
    } catch [System.IO.IOException] {
        throw "local release pipeline is already running. Lock: $LockPath"
    }

    return $token
}

# Remove only the lock created by this process; a failed contender must not delete another run's lock.
function Exit-PipelineLock {
    param(
        [string]$LockPath,
        [string]$Token
    )

    if ([string]::IsNullOrWhiteSpace($Token) -or -not (Test-Path -LiteralPath $LockPath)) {
        return
    }

    $owner = (Get-Content -LiteralPath $LockPath -TotalCount 1 -ErrorAction SilentlyContinue).Trim()
    if ($owner -eq $Token) {
        Remove-Item -LiteralPath $LockPath -Force -ErrorAction SilentlyContinue
    }
}

# Write release metadata into the export folder so the artifact can be traced to a commit.
function Write-ReleaseMetadata {
    param(
        [string]$Root,
        [string]$Reason,
        [Parameter(Mandatory = $true)][psobject]$Identity
    )

    $metadata = @(
        "# Local Release Metadata",
        "",
        "Reason: $Reason",
        "Branch: $($Identity.Branch)",
        "Commit: $($Identity.Commit)",
        "Version: $($Identity.Version)",
        "GeneratedAt: $(Get-Date -Format o)"
    ) -join "`r`n"

    [System.IO.File]::WriteAllText(
        (Join-Path $Root "RELEASE-METADATA.md"),
        $metadata,
        [System.Text.UTF8Encoding]::new($false)
    )
}

# Recompute checksums after metadata is written.
function Write-Checksums {
    param([string]$Root)

    $normalizedRoot = [System.IO.Path]::GetFullPath($Root)
    $lines = New-Object System.Collections.Generic.List[string]
    Get-ChildItem -LiteralPath $normalizedRoot -Recurse -File |
        Where-Object { $_.Name -ne "SHA256SUMS.txt" } |
        ForEach-Object {
        $file = $_
        $hash = Get-ReleaseFileSha256 -Path $file.FullName
        $relative = $file.FullName.Substring($normalizedRoot.Length).TrimStart([char[]]@([char]92, [char]47))
        $lines.Add("$hash  $relative")
    } | Out-Null
    [System.IO.File]::WriteAllText(
        (Join-Path $normalizedRoot "SHA256SUMS.txt"),
        ($lines.ToArray() -join "`r`n"),
        [System.Text.UTF8Encoding]::new($false)
    )
}

$repoRoot = Get-RepoRoot
$releaseRoot = [System.IO.Path]::GetFullPath(
    (Resolve-CcswitchmultiReleaseRoot -RepoRoot $repoRoot -RequestedRoot $ReleaseRoot)
)
$logDir = Join-Path $repoRoot "scripts\logs"
$lockPath = Join-Path $logDir "local-release.lock"

New-Item -ItemType Directory -Force -Path $logDir | Out-Null

$pipelineLockToken = $null
$stageRoot = $null
try {
    $pipelineLockToken = Enter-PipelineLock -LockPath $lockPath
    Push-Location $repoRoot

    Write-Log "Local release pipeline started. reason=$Reason target=$releaseRoot"
    $sourceIdentity = Get-ReleaseSourceIdentity -RepoRoot $repoRoot
    if ($sourceIdentity.TrackedWorktree -ne "clean") {
        throw "local release requires a clean tracked worktree; commit or stash tracked changes before building"
    }
    $stageRoot = New-ReleaseStageRoot -ReleaseRoot $releaseRoot
    Assert-ReleaseStagePair -StageRoot $stageRoot -ReleaseRoot $releaseRoot

    Invoke-CheckedCommand -FilePath "pnpm" -Arguments @("install", "--frozen-lockfile", "--force")
    Assert-LocalTauriCliVersion -RepoRoot $repoRoot
    Assert-ReleaseSourceIdentity `
        -Expected $sourceIdentity `
        -Actual (Get-ReleaseSourceIdentity -RepoRoot $repoRoot)

    if (-not $NoTypecheck) {
        Invoke-CheckedCommand -FilePath "pnpm" -Arguments @("typecheck")
    }

    $exportArgs = @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        "scripts/export-latest-ccswitchmulti.ps1",
        "-ReleaseRoot",
        $stageRoot
    )
    if ($SkipBuild) {
        $exportArgs += "-SkipBuild"
    }

    Invoke-CheckedCommand -FilePath "powershell" -Arguments $exportArgs
    Assert-ReleaseSourceIdentity `
        -Expected $sourceIdentity `
        -Actual (Get-ReleaseSourceIdentity -RepoRoot $repoRoot)
    Write-ReleaseMetadata -Root $stageRoot -Reason $Reason -Identity $sourceIdentity
    Write-Checksums -Root $stageRoot
    Assert-ReleaseSourceIdentity `
        -Expected $sourceIdentity `
        -Actual (Get-ReleaseSourceIdentity -RepoRoot $repoRoot)
    Replace-ReleaseRootFromStage -StageRoot $stageRoot -ReleaseRoot $releaseRoot
    $stageRoot = $null

    Write-Log "Local release pipeline completed. Artifacts exported to: $releaseRoot"
} finally {
    Pop-Location -ErrorAction SilentlyContinue
    if ($stageRoot -and (Test-Path -LiteralPath $stageRoot)) {
        Remove-Item -LiteralPath $stageRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    Exit-PipelineLock -LockPath $lockPath -Token $pipelineLockToken
}
