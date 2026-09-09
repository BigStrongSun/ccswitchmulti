param(
    [string]$RepoRoot = "",
    [double]$MinimumAgeHours = 6,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $PSCommandPath
. (Join-Path $scriptDir "cargo-cache-cleanup-lib.ps1")

if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
    $RepoRoot = Resolve-Path (Join-Path $scriptDir "..") | Select-Object -ExpandProperty Path
}
$repoFull = [System.IO.Path]::GetFullPath($RepoRoot)
$porcelain = @(& git -C $repoFull worktree list --porcelain 2>$null)
if ($LASTEXITCODE -ne 0) {
    throw "cannot enumerate registered Git worktrees from: $repoFull"
}
$worktreeRoots = @(Get-CcswitchmultiRegisteredWorktreeRoots -PorcelainLines $porcelain)
if ($worktreeRoots.Count -eq 0) {
    throw "Git returned no registered worktrees from: $repoFull"
}

$buildProcesses = @()
if (-not $DryRun) {
    try {
        $buildProcesses = @(Get-CimInstance Win32_Process | Where-Object {
                $_.Name -match '^(cargo|rustc)(\.exe)?$'
            })
    } catch {
        throw "cannot inspect Cargo/rustc processes; cleanup refused: $($_.Exception.Message)"
    }
}

$result = Invoke-CcswitchmultiCargoCacheCleanup `
    -WorktreeRoots $worktreeRoots `
    -MinimumAgeHours $MinimumAgeHours `
    -BuildProcesses $buildProcesses `
    -DryRun:$DryRun

foreach ($candidate in $result.Candidates) {
    $mode = if ($DryRun) { "DRY-RUN" } elseif ($result.SkippedForActiveBuild) { "SKIPPED" } else { "SELECTED" }
    Write-Host "$mode $($candidate.FullName)"
}
if ($result.SkippedForActiveBuild) {
    Write-Warning "Cargo/rustc is active; the entire destructive cleanup pass was skipped."
}
Write-Host (
    "Cargo cache cleanup summary: worktrees={0} candidates={1} removed={2} failed={3} dryRun={4} skippedForActiveBuild={5}" -f
    $worktreeRoots.Count,
    $result.CandidateCount,
    $result.RemovedCount,
    $result.FailedCount,
    $DryRun.IsPresent,
    $result.SkippedForActiveBuild
)
if ($result.FailedCount -gt 0) {
    throw "one or more Cargo target directories could not be cleaned"
}
