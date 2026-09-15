function Get-CcswitchmultiRegisteredWorktreeRoots {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string[]]$PorcelainLines)

    $roots = foreach ($line in $PorcelainLines) {
        if (-not $line.StartsWith("worktree ", [System.StringComparison]::Ordinal)) {
            continue
        }
        $path = $line.Substring("worktree ".Length)
        if (-not [string]::IsNullOrWhiteSpace($path)) {
            [System.IO.Path]::GetFullPath($path)
        }
    }
    return @($roots)
}

function Assert-CcswitchmultiCargoTargetPath {
    param(
        [Parameter(Mandatory = $true)][string]$WorktreeRoot,
        [Parameter(Mandatory = $true)][string]$TargetDir
    )

    $worktreeFull = [System.IO.Path]::GetFullPath($WorktreeRoot)
    $srcTauriFull = [System.IO.Path]::GetFullPath((Join-Path $worktreeFull "src-tauri"))
    $targetFull = [System.IO.Path]::GetFullPath($TargetDir)
    $targetParent = [System.IO.Path]::GetFullPath((Split-Path -Parent $targetFull))
    $targetName = [System.IO.Path]::GetFileName($targetFull.TrimEnd('\', '/'))
    $isTargetName = $targetName -eq "target" -or $targetName.StartsWith(
            "target-",
            [System.StringComparison]::OrdinalIgnoreCase
        )
    $isSrcTauriTarget = $isTargetName -and [string]::Equals(
        $targetParent,
        $srcTauriFull,
        [System.StringComparison]::OrdinalIgnoreCase
    )
    $isReleaseTarget = $targetName -eq ".release-target" -and [string]::Equals(
        $targetParent,
        $worktreeFull,
        [System.StringComparison]::OrdinalIgnoreCase
    )
    $sddRoot = [System.IO.Path]::GetFullPath((Join-Path $worktreeFull ".superpowers\sdd"))
    $sddTaskRoot = [System.IO.Path]::GetFullPath($targetParent)
    $sddTaskParent = [System.IO.Path]::GetFullPath((Split-Path -Parent $sddTaskRoot))
    $isSddTarget = $isTargetName -and [string]::Equals(
        $sddTaskParent,
        $sddRoot,
        [System.StringComparison]::OrdinalIgnoreCase
    )
    if (-not ($isSrcTauriTarget -or $isReleaseTarget -or $isSddTarget)) {
        throw "Cargo cleanup target must be a direct child of the worktree src-tauri directory or a supported release/SDD Cargo target: $targetFull"
    }
    if (-not $isReleaseTarget -and -not $isTargetName) {
        throw "Cargo cleanup target name must be target or target-*: $targetFull"
    }
    if (-not (Test-Path -LiteralPath $targetFull -PathType Container)) {
        throw "Cargo cleanup target directory is missing: $targetFull"
    }
    if ($isSddTarget) {
        foreach ($containerPath in @($sddRoot, $sddTaskRoot)) {
            $containerItem = Get-Item -LiteralPath $containerPath -Force
            if (($containerItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Cargo cleanup SDD parent must not be a reparse point: $containerPath"
            }
        }
    }
    $targetItem = Get-Item -LiteralPath $targetFull -Force
    if (($targetItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Cargo cleanup target must not be a reparse point: $targetFull"
    }

    return $targetFull
}

function Get-CcswitchmultiStaleCargoTargetDirectories {
    param(
        [Parameter(Mandatory = $true)][string[]]$WorktreeRoots,
        [Parameter(Mandatory = $true)][double]$MinimumAgeHours,
        [datetime]$Now = (Get-Date)
    )

    if ($MinimumAgeHours -lt 0) {
        throw "MinimumAgeHours must be zero or greater"
    }

    $cutoff = $Now.AddHours(-$MinimumAgeHours)
    $candidates = foreach ($worktreeRoot in $WorktreeRoots) {
        $worktreeFull = [System.IO.Path]::GetFullPath($worktreeRoot)
        $supportedTargets = New-Object System.Collections.Generic.List[object]
        $srcTauri = Join-Path $worktreeFull "src-tauri"
        if (Test-Path -LiteralPath $srcTauri -PathType Container) {
            foreach ($target in (Get-ChildItem -LiteralPath $srcTauri -Directory -Force -ErrorAction SilentlyContinue)) {
                if ($target.Name -eq "target" -or $target.Name.StartsWith(
                        "target-",
                        [System.StringComparison]::OrdinalIgnoreCase
                    )) {
                    $supportedTargets.Add($target)
                }
            }
        }

        $releaseTarget = Join-Path $worktreeFull ".release-target"
        if (Test-Path -LiteralPath $releaseTarget -PathType Container) {
            $supportedTargets.Add((Get-Item -LiteralPath $releaseTarget -Force))
        }

        $sddRoot = Join-Path $worktreeFull ".superpowers\sdd"
        if (Test-Path -LiteralPath $sddRoot -PathType Container) {
            $sddRootItem = Get-Item -LiteralPath $sddRoot -Force
            if (($sddRootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                continue
            }
            foreach ($taskRoot in (Get-ChildItem -LiteralPath $sddRoot -Directory -Force -ErrorAction SilentlyContinue)) {
                if (($taskRoot.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                    continue
                }
                foreach ($target in (Get-ChildItem -LiteralPath $taskRoot.FullName -Directory -Force -ErrorAction SilentlyContinue)) {
                    if ($target.Name -eq "target" -or $target.Name.StartsWith(
                            "target-",
                            [System.StringComparison]::OrdinalIgnoreCase
                        )) {
                        $supportedTargets.Add($target)
                    }
                }
            }
        }

        foreach ($target in $supportedTargets) {
            if ($target.LastWriteTime -gt $cutoff) {
                continue
            }
            if (($target.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                continue
            }

            [pscustomobject]@{
                FullName = [System.IO.Path]::GetFullPath($target.FullName)
                WorktreeRoot = $worktreeFull
                LastWriteTime = $target.LastWriteTime
            }
        }
    }

    return @($candidates | Sort-Object FullName)
}

function Test-CcswitchmultiCargoBuildActive {
    param([object[]]$BuildProcesses = @())

    return @($BuildProcesses | Where-Object {
            $_.Name -match '^(cargo|rustc)(\.exe)?$'
        }).Count -gt 0
}

function Remove-CcswitchmultiCargoTargetDir {
    param(
        [Parameter(Mandatory = $true)][string]$WorktreeRoot,
        [Parameter(Mandatory = $true)][string]$TargetDir
    )

    $targetFull = Assert-CcswitchmultiCargoTargetPath -WorktreeRoot $WorktreeRoot -TargetDir $TargetDir
    $manifestPath = Join-Path ([System.IO.Path]::GetFullPath($WorktreeRoot)) "src-tauri\Cargo.toml"
    if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
        throw "Cargo manifest is missing for cleanup target: $manifestPath"
    }

    & cargo clean --manifest-path $manifestPath --target-dir $targetFull 2>&1 |
        ForEach-Object { Write-Host ([string]$_) }
    $cargoExitCode = $LASTEXITCODE
    if ($cargoExitCode -ne 0) {
        throw "cargo clean failed with exit code $cargoExitCode for target: $targetFull"
    }
}

function Invoke-CcswitchmultiCargoCacheCleanup {
    param(
        [Parameter(Mandatory = $true)][string[]]$WorktreeRoots,
        [double]$MinimumAgeHours = 6,
        [object[]]$BuildProcesses = @(),
        [switch]$DryRun
    )

    $candidates = @(Get-CcswitchmultiStaleCargoTargetDirectories `
            -WorktreeRoots $WorktreeRoots `
            -MinimumAgeHours $MinimumAgeHours)
    if (-not $DryRun -and (Test-CcswitchmultiCargoBuildActive -BuildProcesses $BuildProcesses)) {
        return [pscustomobject]@{
            SkippedForActiveBuild = $true
            CandidateCount = $candidates.Count
            RemovedCount = 0
            FailedCount = 0
            Candidates = $candidates
        }
    }

    $removedCount = 0
    $failedCount = 0
    if (-not $DryRun) {
        foreach ($candidate in $candidates) {
            try {
                Remove-CcswitchmultiCargoTargetDir `
                    -WorktreeRoot $candidate.WorktreeRoot `
                    -TargetDir $candidate.FullName
                $removedCount++
            } catch {
                $failedCount++
                Write-Warning $_.Exception.Message
            }
        }
    }

    return [pscustomobject]@{
        SkippedForActiveBuild = $false
        CandidateCount = $candidates.Count
        RemovedCount = $removedCount
        FailedCount = $failedCount
        Candidates = $candidates
    }
}
