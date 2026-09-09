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

    It "discovers only stale direct target and target-dash directories" {
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
            [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "targeted")) | Out-Null
            [System.IO.Directory]::CreateDirectory((Join-Path $srcTauri "nested\target-old")) | Out-Null
            $now = [datetime]'2026-09-09T18:00:00'
            $staleDefault.LastWriteTime = $now.AddHours(-8)
            $staleCustom.LastWriteTime = $now.AddHours(-7)
            $recent.LastWriteTime = $now.AddHours(-1)

            $found = @(Get-CcswitchmultiStaleCargoTargetDirectories `
                    -WorktreeRoots @($fixtureRoot) `
                    -MinimumAgeHours 6 `
                    -Now $now)

            $found.Count | Should Be 2
            $found[0].FullName | Should Be ([System.IO.Path]::GetFullPath($staleDefault.FullName))
            $found[1].FullName | Should Be ([System.IO.Path]::GetFullPath($staleCustom.FullName))
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
