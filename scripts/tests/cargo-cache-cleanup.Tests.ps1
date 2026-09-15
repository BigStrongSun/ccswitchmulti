$libraryPath = Join-Path (Split-Path -Parent $PSScriptRoot) "cargo-cache-cleanup-lib.ps1"

Describe "CCSwitchMulti Cargo cache cleanup" {
    It "parses every registered worktree path without splitting spaces" {
        (Test-Path -LiteralPath $libraryPath -PathType Leaf) | Should Be $true
        if (-not (Test-Path -LiteralPath $libraryPath -PathType Leaf)) {
            return
        }
        . $libraryPath

        $roots = @(Get-CcswitchmultiRegisteredWorktreeRoots -PorcelainLines @(
                'worktree C:/workspace/cc-switch',
                'HEAD 1111111111111111111111111111111111111111',
                'branch refs/heads/main',
                '',
                'worktree C:/workspace/feature worktree',
                'HEAD 2222222222222222222222222222222222222222',
                'detached'
            ))

        $roots.Count | Should Be 2
        $roots[0] | Should Be ([System.IO.Path]::GetFullPath('C:/workspace/cc-switch'))
        $roots[1] | Should Be ([System.IO.Path]::GetFullPath('C:/workspace/feature worktree'))
    }

    It "discovers stale Cargo targets from supported build and diagnostic locations" {
        (Test-Path -LiteralPath $libraryPath -PathType Leaf) | Should Be $true
        if (-not (Test-Path -LiteralPath $libraryPath -PathType Leaf)) {
            return
        }
        . $libraryPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-cache-discovery-" + [guid]::NewGuid().ToString("N"))
        $srcTauri = Join-Path $fixtureRoot "src-tauri"
        [System.IO.Directory]::CreateDirectory($srcTauri) | Out-Null
        try {
            $staleDefault = [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "target"))
            $staleCustom = [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "target-history-exclusive"))
            $recent = [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "target-recent"))
            $releaseTarget = [System.IO.Directory]::CreateDirectory((Join-Path $fixtureRoot ".release-target"))
            $sddTaskRoot = Join-Path $fixtureRoot ".superpowers\sdd\2026-09-09-diagnostic"
            $sddTarget = [System.IO.Directory]::CreateDirectory((Join-Path $sddTaskRoot "target"))
            $sddCustom = [System.IO.Directory]::CreateDirectory((Join-Path $sddTaskRoot "target-flake-diagnosis"))
            $sddTooDeep = [System.IO.Directory]::CreateDirectory((Join-Path $sddTaskRoot "nested\target"))
            [System.IO.Directory]::CreateDirectory((Join-Path $sddTaskRoot "evidence")) | Out-Null
            $rootTarget = [System.IO.Directory]::CreateDirectory((Join-Path $fixtureRoot "target"))
            [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "targeted")) | Out-Null
            [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "nested\target-old")) | Out-Null
            $now = [datetime]'2026-09-09T18:00:00'
            $staleDefault.LastWriteTime = $now.AddHours(-8)
            $staleCustom.LastWriteTime = $now.AddHours(-7)
            $recent.LastWriteTime = $now.AddHours(-1)
            $releaseTarget.LastWriteTime = $now.AddHours(-8)
            $sddTarget.LastWriteTime = $now.AddHours(-8)
            $sddCustom.LastWriteTime = $now.AddHours(-8)
            $sddTooDeep.LastWriteTime = $now.AddHours(-8)
            $rootTarget.LastWriteTime = $now.AddHours(-8)

            $found = @(Get-CcswitchmultiStaleCargoTargetDirectories `
                    -WorktreeRoots @($fixtureRoot) `
                    -MinimumAgeHours 6 `
                    -Now $now)

            $found.Count | Should Be 5
            (@($found.FullName) -contains ([System.IO.Path]::GetFullPath($staleDefault.FullName))) | Should Be $true
            (@($found.FullName) -contains ([System.IO.Path]::GetFullPath($staleCustom.FullName))) | Should Be $true
            (@($found.FullName) -contains ([System.IO.Path]::GetFullPath($releaseTarget.FullName))) | Should Be $true
            (@($found.FullName) -contains ([System.IO.Path]::GetFullPath($sddTarget.FullName))) | Should Be $true
            (@($found.FullName) -contains ([System.IO.Path]::GetFullPath($sddCustom.FullName))) | Should Be $true
            (@($found.FullName) -contains ([System.IO.Path]::GetFullPath($sddTooDeep.FullName))) | Should Be $false
            (@($found.FullName) -contains ([System.IO.Path]::GetFullPath($rootTarget.FullName))) | Should Be $false
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "accepts only the exact worktree release target and one-level SDD target shapes" {
        (Test-Path -LiteralPath $libraryPath -PathType Leaf) | Should Be $true
        if (-not (Test-Path -LiteralPath $libraryPath -PathType Leaf)) {
            return
        }
        . $libraryPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-cache-supported-boundary-" + [guid]::NewGuid().ToString("N"))
        $releaseTarget = Join-Path $fixtureRoot ".release-target"
        $sddTarget = Join-Path $fixtureRoot ".superpowers\sdd\diagnostic\target-flake"
        $tooDeep = Join-Path $fixtureRoot ".superpowers\sdd\diagnostic\nested\target"
        $rootTarget = Join-Path $fixtureRoot "target"
        foreach ($path in @($releaseTarget, $sddTarget, $tooDeep, $rootTarget)) {
            [System.IO.Directory]::CreateDirectory($path) | Out-Null
        }
        try {
            Assert-CcswitchmultiCargoTargetPath -WorktreeRoot $fixtureRoot -TargetDir $releaseTarget |
                Should Be ([System.IO.Path]::GetFullPath($releaseTarget))
            Assert-CcswitchmultiCargoTargetPath -WorktreeRoot $fixtureRoot -TargetDir $sddTarget |
                Should Be ([System.IO.Path]::GetFullPath($sddTarget))
            { Assert-CcswitchmultiCargoTargetPath -WorktreeRoot $fixtureRoot -TargetDir $tooDeep } |
                Should Throw "supported release/SDD Cargo target"
            { Assert-CcswitchmultiCargoTargetPath -WorktreeRoot $fixtureRoot -TargetDir $rootTarget } |
                Should Throw "supported release/SDD Cargo target"
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "rejects a Cargo cleanup target outside the selected worktree src-tauri directory" {
        (Test-Path -LiteralPath $libraryPath -PathType Leaf) | Should Be $true
        if (-not (Test-Path -LiteralPath $libraryPath -PathType Leaf)) {
            return
        }
        . $libraryPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-cache-boundary-" + [guid]::NewGuid().ToString("N"))
        $outside = Join-Path $fixtureRoot "target"
        [System.IO.Directory]::CreateDirectory((Join-Path $fixtureRoot "src-tauri")) | Out-Null
        [System.IO.Directory]::CreateDirectory($outside) | Out-Null
        try {
            { Assert-CcswitchmultiCargoTargetPath -WorktreeRoot $fixtureRoot -TargetDir $outside } |
                Should Throw "must be a direct child of the worktree src-tauri directory"
            (Test-Path -LiteralPath $outside -PathType Container) | Should Be $true
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "skips the entire destructive pass while Cargo or rustc is active" {
        (Test-Path -LiteralPath $libraryPath -PathType Leaf) | Should Be $true
        if (-not (Test-Path -LiteralPath $libraryPath -PathType Leaf)) {
            return
        }
        . $libraryPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-cache-active-" + [guid]::NewGuid().ToString("N"))
        $target = Join-Path $fixtureRoot "src-tauri\target"
        [System.IO.Directory]::CreateDirectory($target) | Out-Null
        try {
            (Get-Item -LiteralPath $target).LastWriteTime = (Get-Date).AddDays(-1)
            $result = Invoke-CcswitchmultiCargoCacheCleanup `
                -WorktreeRoots @($fixtureRoot) `
                -MinimumAgeHours 0 `
                -BuildProcesses @([pscustomobject]@{ Name = "cargo.exe" })

            $result.SkippedForActiveBuild | Should Be $true
            $result.RemovedCount | Should Be 0
            (Test-Path -LiteralPath $target -PathType Container) | Should Be $true
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "uses Cargo to remove a validated generated target directory" {
        (Test-Path -LiteralPath $libraryPath -PathType Leaf) | Should Be $true
        if (-not (Test-Path -LiteralPath $libraryPath -PathType Leaf)) {
            return
        }
        . $libraryPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-cache-clean-" + [guid]::NewGuid().ToString("N"))
        $srcTauri = Join-Path $fixtureRoot "src-tauri"
        $target = Join-Path $srcTauri "target-old"
        [System.IO.Directory]::CreateDirectory($target) | Out-Null
        try {
            [System.IO.File]::WriteAllText(
                (Join-Path $srcTauri "Cargo.toml"),
                "[package]`nname = `"cache-fixture`"`nversion = `"0.1.0`"`nedition = `"2021`"`n",
                [System.Text.UTF8Encoding]::new($false)
            )
            [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "src")) | Out-Null
            [System.IO.File]::WriteAllText((Join-Path $srcTauri "src\lib.rs"), "pub fn fixture() {}")
            [System.IO.File]::WriteAllText((Join-Path $target "generated.bin"), "generated")

            Remove-CcswitchmultiCargoTargetDir -WorktreeRoot $fixtureRoot -TargetDir $target

            (Test-Path -LiteralPath $target) | Should Be $false
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }
}
