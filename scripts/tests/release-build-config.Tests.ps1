$helperPath = Join-Path (Split-Path -Parent $PSScriptRoot) "release-build-config.ps1"

Describe "CCSwitchMulti local release build config" {
    function Get-CargoPackageVersion {
        param(
            [string]$CargoLock,
            [string]$PackageName
        )

        $match = [regex]::Match(
            $CargoLock,
            "(?ms)^name = `"$([regex]::Escape($PackageName))`"\r?\nversion = `"(?<version>[^`"]+)`""
        )
        $match.Success | Should Be $true
        return [version]$match.Groups["version"].Value
    }

    It "pins a Tauri CLI that understands marker-based tauri-utils bundle metadata" {
        $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
        $packageJson = [System.IO.File]::ReadAllText((Join-Path $repoRoot "package.json")) | ConvertFrom-Json
        $cargoLock = [System.IO.File]::ReadAllText((Join-Path $repoRoot "src-tauri\Cargo.lock"))

        $tauriUtilsMatch = [regex]::Match(
            $cargoLock,
            '(?ms)^name = "tauri-utils"\r?\nversion = "(?<version>[^"]+)"'
        )
        $tauriUtilsMatch.Success | Should Be $true

        $tauriUtilsVersion = [version]$tauriUtilsMatch.Groups["version"].Value
        $tauriCliRequirement = [string]$packageJson.devDependencies.'@tauri-apps/cli'
        $tauriCliVersion = [version]($tauriCliRequirement.TrimStart('^', '~', '=', ' '))

        if ($tauriUtilsVersion -ge [version]'2.8.3') {
            $tauriCliVersion -ge [version]'2.10.1' | Should Be $true
        }
    }

    It "keeps Tauri JavaScript bindings on the same major and minor release as Rust" {
        $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
        $packageJson = [System.IO.File]::ReadAllText((Join-Path $repoRoot "package.json")) | ConvertFrom-Json
        $cargoLock = [System.IO.File]::ReadAllText((Join-Path $repoRoot "src-tauri\Cargo.lock"))
        $pairs = @(
            @{ Rust = 'tauri'; JavaScript = '@tauri-apps/api' },
            @{ Rust = 'tauri-plugin-dialog'; JavaScript = '@tauri-apps/plugin-dialog' },
            @{ Rust = 'tauri-plugin-updater'; JavaScript = '@tauri-apps/plugin-updater' }
        )

        foreach ($pair in $pairs) {
            $rustVersion = Get-CargoPackageVersion -CargoLock $cargoLock -PackageName $pair.Rust
            $javascriptRequirement = [string]$packageJson.dependencies.($pair.JavaScript)
            $javascriptRequirement | Should Match '^\d+\.\d+\.\d+$'
            $javascriptVersion = [version]$javascriptRequirement

            $javascriptVersion.Major | Should Be $rustVersion.Major
            $javascriptVersion.Minor | Should Be $rustVersion.Minor
        }
    }

    It "rejects a stale installed Tauri CLI before any local release build" {
        . $helperPath

        $command = Get-Command Assert-LocalTauriCliVersion -ErrorAction SilentlyContinue
        $command | Should Not BeNullOrEmpty
        if ($null -eq $command) {
            return
        }

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-tauri-cli-" + [guid]::NewGuid().ToString("N"))
        $installedPackageRoot = Join-Path $fixtureRoot "node_modules\@tauri-apps\cli"
        [System.IO.Directory]::CreateDirectory($installedPackageRoot) | Out-Null
        try {
            [System.IO.File]::WriteAllText(
                (Join-Path $fixtureRoot "package.json"),
                '{"devDependencies":{"@tauri-apps/cli":"2.10.1"}}',
                [System.Text.UTF8Encoding]::new($false)
            )
            [System.IO.File]::WriteAllText(
                (Join-Path $installedPackageRoot "package.json"),
                '{"version":"2.8.1"}',
                [System.Text.UTF8Encoding]::new($false)
            )

            { Assert-LocalTauriCliVersion -RepoRoot $fixtureRoot } |
                Should Throw "installed Tauri CLI package version mismatch"
        } finally {
            [System.IO.Directory]::Delete($fixtureRoot, $true)
        }
    }

    It "runs a frozen dependency install before validating and building a local release" {
        $pipelinePath = Join-Path (Split-Path -Parent $helperPath) "local-release-pipeline.ps1"
        $pipeline = [System.IO.File]::ReadAllText($pipelinePath)
        $installOffset = $pipeline.IndexOf('Invoke-CheckedCommand -FilePath "pnpm" -Arguments @("install", "--frozen-lockfile", "--force")')
        $assertOffset = $pipeline.IndexOf("Assert-LocalTauriCliVersion -RepoRoot `$repoRoot")
        $exportOffset = $pipeline.IndexOf('Invoke-CheckedCommand -FilePath "powershell" -Arguments $exportArgs')

        $installOffset | Should BeGreaterThan -1
        $assertOffset | Should BeGreaterThan $installOffset
        $exportOffset | Should BeGreaterThan $assertOffset
    }

    It "captures source identity and exports to staging before replacing the final release root" {
        $pipelinePath = Join-Path (Split-Path -Parent $helperPath) "local-release-pipeline.ps1"
        $pipeline = [System.IO.File]::ReadAllText($pipelinePath)
        $captureOffset = $pipeline.IndexOf('$sourceIdentity = Get-ReleaseSourceIdentity -RepoRoot $repoRoot')
        $stageOffset = $pipeline.IndexOf('"-ReleaseRoot",')
        $stageVariableOffset = $pipeline.IndexOf('$stageRoot', $stageOffset)
        $postExportGuardOffset = $pipeline.IndexOf('Assert-ReleaseSourceIdentity', $pipeline.IndexOf('Invoke-CheckedCommand -FilePath "powershell"'))
        $swapOffset = $pipeline.IndexOf('Replace-ReleaseRootFromStage')

        $captureOffset | Should BeGreaterThan -1
        $stageOffset | Should BeGreaterThan $captureOffset
        $stageVariableOffset | Should BeGreaterThan $stageOffset
        $postExportGuardOffset | Should BeGreaterThan $stageVariableOffset
        $swapOffset | Should BeGreaterThan $postExportGuardOffset
    }

    It "rejects a release when the tracked source identity changes during the build" {
        . $helperPath

        $expected = [pscustomobject]@{
            Commit = "commit-a"
            Branch = "main"
            Version = "3.19.2-12"
            TrackedWorktree = "clean"
        }
        $actual = [pscustomobject]@{
            Commit = "commit-b"
            Branch = "main"
            Version = "3.19.2-12"
            TrackedWorktree = "clean"
        }

        { Assert-ReleaseSourceIdentity -Expected $expected -Actual $actual } |
            Should Throw "release source identity changed"
    }

    It "accepts an unchanged release source identity" {
        . $helperPath

        $identity = [pscustomobject]@{
            Commit = "commit-a"
            Branch = "main"
            Version = "3.19.2-12"
            TrackedWorktree = "clean"
        }

        { Assert-ReleaseSourceIdentity -Expected $identity -Actual $identity } |
            Should Not Throw
    }

    It "swaps a validated sibling release staging directory into place" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-swap-" + [guid]::NewGuid().ToString("N"))
        $releaseRoot = Join-Path $fixtureRoot "final"
        $stageRoot = Join-Path $fixtureRoot "stage"
        New-Item -ItemType Directory -Force -Path $releaseRoot, $stageRoot | Out-Null
        try {
            [System.IO.File]::WriteAllText((Join-Path $releaseRoot "marker.txt"), "old")
            [System.IO.File]::WriteAllText((Join-Path $stageRoot "marker.txt"), "new")

            Replace-ReleaseRootFromStage -StageRoot $stageRoot -ReleaseRoot $releaseRoot

            [System.IO.File]::ReadAllText((Join-Path $releaseRoot "marker.txt")) |
                Should Be "new"
            (Test-Path -LiteralPath $stageRoot) | Should Be $false
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                Remove-Item -LiteralPath $fixtureRoot -Recurse -Force
            }
        }
    }

    It "rejects a release staging directory outside the final release parent" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-path-" + [guid]::NewGuid().ToString("N"))
        $releaseRoot = Join-Path $fixtureRoot "final"
        $stageRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-stage-" + [guid]::NewGuid().ToString("N"))
        New-Item -ItemType Directory -Force -Path $releaseRoot, $stageRoot | Out-Null
        try {
            {
                Assert-ReleaseStagePair -StageRoot $stageRoot -ReleaseRoot $releaseRoot
            } | Should Throw "release staging path must be a sibling"
        } finally {
            foreach ($path in @($fixtureRoot, $stageRoot)) {
                if (Test-Path -LiteralPath $path) {
                    Remove-Item -LiteralPath $path -Recurse -Force
                }
            }
        }
    }

    It "creates a BOM-free Tauri override without PowerShell utility cmdlets and always supports cleanup" {
        $helperExists = Test-Path -LiteralPath $helperPath
        $helperExists | Should Be $true
        if (-not $helperExists) {
            return
        }

        . $helperPath

        $configPath = New-TauriBuildConfigFile
        try {
            (Test-Path -LiteralPath $configPath) | Should Be $true

            $bytes = [System.IO.File]::ReadAllBytes($configPath)
            $hasUtf8Bom = $bytes.Length -ge 3 -and
                $bytes[0] -eq 0xEF -and
                $bytes[1] -eq 0xBB -and
                $bytes[2] -eq 0xBF
            $hasUtf8Bom | Should Be $false

            $config = [System.IO.File]::ReadAllText($configPath) | ConvertFrom-Json
            $config.bundle.createUpdaterArtifacts | Should Be $false
        } finally {
            Remove-TauriBuildConfigFile -Path $configPath
        }

        (Test-Path -LiteralPath $configPath) | Should Be $false
    }

    It "creates each local release Cargo target inside the repository-owned cache root" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-target-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
        try {
            $first = New-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot
            $second = New-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot
            $expectedRoot = [System.IO.Path]::GetFullPath((Join-Path $fixtureRoot ".tmp\local-release-cargo"))

            (Split-Path -Parent $first) | Should Be $expectedRoot
            (Split-Path -Parent $second) | Should Be $expectedRoot
            $first | Should Not Be $second
            (Test-Path -LiteralPath $first -PathType Container) | Should Be $true
            (Test-Path -LiteralPath $second -PathType Container) | Should Be $true
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "removes only the selected repository-owned local release Cargo target" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-clean-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
        try {
            [System.IO.Directory]::CreateDirectory((Join-Path $fixtureRoot "src-tauri\src")) | Out-Null
            [System.IO.File]::WriteAllText(
                (Join-Path $fixtureRoot "src-tauri\Cargo.toml"),
                "[package]`nname = `"release-clean-fixture`"`nversion = `"0.1.0`"`nedition = `"2021`"`n",
                [System.Text.UTF8Encoding]::new($false)
            )
            [System.IO.File]::WriteAllText((Join-Path $fixtureRoot "src-tauri\src\lib.rs"), "pub fn fixture() {}")
            $target = New-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot
            $sibling = New-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot
            [System.IO.File]::WriteAllText((Join-Path $target "artifact.bin"), "generated")
            [System.IO.File]::WriteAllText((Join-Path $sibling "keep.bin"), "keep")

            Remove-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot -TargetDir $target

            (Test-Path -LiteralPath $target) | Should Be $false
            (Test-Path -LiteralPath (Join-Path $sibling "keep.bin") -PathType Leaf) | Should Be $true
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "treats Cargo cleanup progress on stderr as success under stop semantics" {
        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-clean-stop-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
        try {
            [System.IO.Directory]::CreateDirectory((Join-Path $fixtureRoot "src-tauri\src")) | Out-Null
            [System.IO.File]::WriteAllText(
                (Join-Path $fixtureRoot "src-tauri\Cargo.toml"),
                "[package]`nname = `"release-clean-stop-fixture`"`nversion = `"0.1.0`"`nedition = `"2021`"`n",
                [System.Text.UTF8Encoding]::new($false)
            )
            [System.IO.File]::WriteAllText((Join-Path $fixtureRoot "src-tauri\src\lib.rs"), "pub fn fixture() {}")
            $target = Join-Path $fixtureRoot ".tmp\local-release-cargo\run-fixture"
            [System.IO.Directory]::CreateDirectory($target) | Out-Null
            [System.IO.File]::WriteAllText((Join-Path $target "artifact.bin"), "generated")
            $scriptPath = Join-Path $fixtureRoot "cleanup.ps1"
            $scriptBody = @"
`$ErrorActionPreference = "Stop"
. '$($helperPath.Replace("'", "''"))'
Remove-LocalReleaseCargoTargetDir -RepoRoot '$($fixtureRoot.Replace("'", "''"))' -TargetDir '$($target.Replace("'", "''"))'
"@
            [System.IO.File]::WriteAllText($scriptPath, $scriptBody, [System.Text.UTF8Encoding]::new($false))

            $windowsPowerShell = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
            $output = & $windowsPowerShell -NoProfile -ExecutionPolicy Bypass -File $scriptPath 2>&1
            $exitCode = $LASTEXITCODE

            $exitCode | Should Be 0
            (Test-Path -LiteralPath $target) | Should Be $false
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "still rejects a real nonzero Cargo cleanup exit" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-clean-fail-" + [guid]::NewGuid().ToString("N"))
        $fakeBin = Join-Path $fixtureRoot "bin"
        $target = Join-Path $fixtureRoot ".tmp\local-release-cargo\run-fixture"
        [System.IO.Directory]::CreateDirectory((Join-Path $fixtureRoot "src-tauri")) | Out-Null
        [System.IO.Directory]::CreateDirectory($fakeBin) | Out-Null
        [System.IO.Directory]::CreateDirectory($target) | Out-Null
        [System.IO.File]::WriteAllText(
            (Join-Path $fixtureRoot "src-tauri\Cargo.toml"),
            "[package]`nname = `"release-clean-fail-fixture`"`nversion = `"0.1.0`"`nedition = `"2021`"`n",
            [System.Text.UTF8Encoding]::new($false)
        )
        [System.IO.File]::WriteAllText(
            (Join-Path $fakeBin "cargo.cmd"),
            "@echo simulated cargo failure 1>&2`r`n@exit /b 7`r`n",
            [System.Text.Encoding]::ASCII
        )
        $previousPath = $env:PATH
        try {
            $env:PATH = "$fakeBin;$previousPath"

            { Remove-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot -TargetDir $target } |
                Should Throw "cargo clean failed with exit code 7 for local release target: $target"
            (Test-Path -LiteralPath $target -PathType Container) | Should Be $true
        } finally {
            $env:PATH = $previousPath
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "rejects local release Cargo cleanup outside the repository-owned cache root" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-boundary-" + [guid]::NewGuid().ToString("N"))
        $outside = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-outside-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
        [System.IO.Directory]::CreateDirectory($outside) | Out-Null
        try {
            [System.IO.File]::WriteAllText((Join-Path $outside "keep.bin"), "keep")

            { Remove-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot -TargetDir $outside } |
                Should Throw "outside the repository-owned local release Cargo cache root"
            (Test-Path -LiteralPath (Join-Path $outside "keep.bin") -PathType Leaf) | Should Be $true
        } finally {
            foreach ($path in @($fixtureRoot, $outside)) {
                if (Test-Path -LiteralPath $path) {
                    [System.IO.Directory]::Delete($path, $true)
                }
            }
        }
    }

    It "rejects local release Cargo cleanup when the repository manifest is missing" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-manifest-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
        try {
            $target = New-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot
            [System.IO.File]::WriteAllText((Join-Path $target "keep.bin"), "keep")

            { Remove-LocalReleaseCargoTargetDir -RepoRoot $fixtureRoot -TargetDir $target } |
                Should Throw "Cargo manifest is missing for local release cleanup"
            (Test-Path -LiteralPath (Join-Path $target "keep.bin") -PathType Leaf) | Should Be $true
        } finally {
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "resolves release artifacts from an explicit Cargo target directory" {
        . $helperPath

        $tauriDir = 'C:\workspace\cc-switch\src-tauri'
        $targetDir = 'D:\ccsm-cache\release-run'

        Resolve-CargoTargetDir -TauriDir $tauriDir -RequestedTargetDir $targetDir |
            Should Be $targetDir
        Resolve-CargoTargetDir -TauriDir $tauriDir -RequestedTargetDir '' |
            Should Be 'C:\workspace\cc-switch\src-tauri\target'
    }

    It "cleans the isolated Cargo target and restores the caller environment after failure" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-lifecycle-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
        [System.IO.Directory]::CreateDirectory((Join-Path $fixtureRoot "src-tauri\src")) | Out-Null
        [System.IO.File]::WriteAllText(
            (Join-Path $fixtureRoot "src-tauri\Cargo.toml"),
            "[package]`nname = `"release-lifecycle-fixture`"`nversion = `"0.1.0`"`nedition = `"2021`"`n",
            [System.Text.UTF8Encoding]::new($false)
        )
        [System.IO.File]::WriteAllText((Join-Path $fixtureRoot "src-tauri\src\lib.rs"), "pub fn fixture() {}")
        $hadPrevious = Test-Path Env:CARGO_TARGET_DIR
        $previous = $env:CARGO_TARGET_DIR
        $env:CARGO_TARGET_DIR = 'D:\caller-owned-cargo-target'
        $script:capturedReleaseTarget = $null
        try {
            {
                Invoke-WithLocalReleaseCargoTarget -RepoRoot $fixtureRoot -Enabled $true -Action {
                    param($targetDir)
                    $script:capturedReleaseTarget = $targetDir
                    [System.IO.File]::WriteAllText((Join-Path $targetDir "partial.bin"), "partial")
                    throw "simulated export failure"
                }
            } | Should Throw "simulated export failure"

            (Test-Path -LiteralPath $script:capturedReleaseTarget) | Should Be $false
            $env:CARGO_TARGET_DIR | Should Be 'D:\caller-owned-cargo-target'
        } finally {
            if ($hadPrevious) {
                $env:CARGO_TARGET_DIR = $previous
            } else {
                Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
            }
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "does not create or override a Cargo target when the release build is skipped" {
        . $helperPath

        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-release-skip-target-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null
        $hadPrevious = Test-Path Env:CARGO_TARGET_DIR
        $previous = $env:CARGO_TARGET_DIR
        $env:CARGO_TARGET_DIR = 'D:\caller-owned-cargo-target'
        try {
            $observed = Invoke-WithLocalReleaseCargoTarget -RepoRoot $fixtureRoot -Enabled $false -Action {
                param($targetDir)
                [pscustomobject]@{
                    TargetDir = $targetDir
                    Environment = $env:CARGO_TARGET_DIR
                }
            }

            $observed.TargetDir | Should BeNullOrEmpty
            $observed.Environment | Should Be 'D:\caller-owned-cargo-target'
            (Test-Path -LiteralPath (Join-Path $fixtureRoot ".tmp\local-release-cargo")) | Should Be $false
        } finally {
            if ($hadPrevious) {
                $env:CARGO_TARGET_DIR = $previous
            } else {
                Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
            }
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "exports release artifacts from the Cargo target selected by the environment" {
        $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
        $fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ccsm-export-target-" + [guid]::NewGuid().ToString("N"))
        $fixtureScripts = Join-Path $fixtureRoot "scripts"
        $fixtureTauri = Join-Path $fixtureRoot "src-tauri"
        $cargoTarget = Join-Path $fixtureRoot ".tmp\selected-cargo-target"
        $releaseDir = Join-Path $cargoTarget "release"
        $bundleDir = Join-Path $releaseDir "bundle\nsis"
        $outputRoot = Join-Path $fixtureRoot "exported"
        $hadPreviousTarget = Test-Path Env:CARGO_TARGET_DIR
        $previousTarget = $env:CARGO_TARGET_DIR
        $previousUserProfile = $env:USERPROFILE
        [System.IO.Directory]::CreateDirectory($fixtureScripts) | Out-Null
        [System.IO.Directory]::CreateDirectory($fixtureTauri) | Out-Null
        [System.IO.Directory]::CreateDirectory($bundleDir) | Out-Null
        [System.IO.Directory]::CreateDirectory((Join-Path $fixtureScripts "codex-history-tool")) | Out-Null
        try {
            Copy-Item -LiteralPath (Join-Path $repoRoot "scripts\export-latest-ccswitchmulti.ps1") -Destination $fixtureScripts
            Copy-Item -LiteralPath (Join-Path $repoRoot "scripts\release-build-config.ps1") -Destination $fixtureScripts
            [System.IO.File]::WriteAllText(
                (Join-Path $fixtureRoot "package.json"),
                '{"version":"9.9.9"}',
                [System.Text.UTF8Encoding]::new($false)
            )
            [System.IO.File]::WriteAllText(
                (Join-Path $bundleDir "CCSwitchMulti_9.9.9_x64-setup.exe"),
                "fixture installer"
            )
            [System.IO.File]::WriteAllBytes(
                (Join-Path $releaseDir "cc-switch.exe"),
                [System.Text.Encoding]::ASCII.GetBytes("before__TAURI_BUNDLE_TYPE_VAR_UNKafter")
            )
            [System.IO.File]::WriteAllText(
                (Join-Path $fixtureScripts "codex-history-tool\codex_history_tool.py"),
                "print('fixture')",
                [System.Text.UTF8Encoding]::new($false)
            )
            $env:CARGO_TARGET_DIR = $cargoTarget
            $env:USERPROFILE = $fixtureRoot

            & (Join-Path $fixtureScripts "export-latest-ccswitchmulti.ps1") -SkipBuild -ReleaseRoot $outputRoot

            (Test-Path -LiteralPath (Join-Path $outputRoot "windows\installer\CCSwitchMulti_9.9.9_x64-setup.exe") -PathType Leaf) |
                Should Be $true
            (Test-Path -LiteralPath (Join-Path $outputRoot "windows\raw-exe\CCSwitchMulti_9.9.9_x64.exe") -PathType Leaf) |
                Should Be $true
        } finally {
            if ($hadPreviousTarget) {
                $env:CARGO_TARGET_DIR = $previousTarget
            } else {
                Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
            }
            $env:USERPROFILE = $previousUserProfile
            if (Test-Path -LiteralPath $fixtureRoot) {
                [System.IO.Directory]::Delete($fixtureRoot, $true)
            }
        }
    }

    It "runs the local release body inside the isolated Cargo target lifecycle" {
        $pipelinePath = Join-Path (Split-Path -Parent $helperPath) "local-release-pipeline.ps1"
        $tokens = $null
        $errors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            $pipelinePath,
            [ref]$tokens,
            [ref]$errors
        )
        $errors.Count | Should Be 0

        $lifecycleCalls = @($ast.FindAll({
                    param($node)
                    $node -is [System.Management.Automation.Language.CommandAst] -and
                    $node.GetCommandName() -eq "Invoke-WithLocalReleaseCargoTarget"
                }, $true))

        $lifecycleCalls.Count | Should Be 1
    }

    It "computes SHA256 without PowerShell utility cmdlets" {
        . $helperPath

        $filePath = [System.IO.Path]::GetTempFileName()
        try {
            [System.IO.File]::WriteAllText(
                $filePath,
                "ccswitchmulti-release",
                [System.Text.UTF8Encoding]::new($false)
            )

            $hash = Get-ReleaseFileSha256 -Path $filePath

            $hash | Should Be "7C10D97BCA5D29117B515F045186B8A7AA535CE7A0F1E775AEC72A1AC9504C2F"
        } finally {
            [System.IO.File]::Delete($filePath)
        }
    }

    It "derives the NSIS-installed executable hash from exactly one restored Tauri marker" {
        . $helperPath

        $filePath = [System.IO.Path]::GetTempFileName()
        try {
            [System.IO.File]::WriteAllBytes(
                $filePath,
                [System.Text.Encoding]::ASCII.GetBytes("before__TAURI_BUNDLE_TYPE_VAR_UNKafter")
            )

            $hash = Get-TauriNsisInstalledExeSha256 -Path $filePath

            $hash | Should Be "2609555DE77DC53CFF714B5AD8D8054D8E7322EDCC395A47828F06E6797695B1"
        } finally {
            [System.IO.File]::Delete($filePath)
        }
    }

    It "rejects raw executables without exactly one restored Tauri marker" {
        . $helperPath

        $filePath = [System.IO.Path]::GetTempFileName()
        try {
            [System.IO.File]::WriteAllBytes(
                $filePath,
                [System.Text.Encoding]::ASCII.GetBytes("no bundle marker")
            )

            {
                Get-TauriNsisInstalledExeSha256 -Path $filePath
            } | Should Throw "raw Tauri executable must contain exactly one restored UNK bundle marker"
        } finally {
            [System.IO.File]::Delete($filePath)
        }
    }

    It "keeps the default export beside the main repository from a linked worktree" {
        . $helperPath

        $repoRoot = 'C:\workspace\cc-switch\.worktrees\feature'
        $gitCommonDir = 'C:\workspace\cc-switch\.git'

        $releaseRoot = Resolve-CcswitchmultiReleaseRoot `
            -RepoRoot $repoRoot `
            -GitCommonDir $gitCommonDir

        $releaseRoot | Should Be 'C:\workspace\最新版ccswitchmulti'
    }

    It "keeps the default export beside a normal main checkout" {
        . $helperPath

        $releaseRoot = Resolve-CcswitchmultiReleaseRoot `
            -RepoRoot 'C:\workspace\cc-switch' `
            -GitCommonDir 'C:\workspace\cc-switch\.git'

        $releaseRoot | Should Be 'C:\workspace\最新版ccswitchmulti'
    }

    It "honors an explicit release root without consulting Git metadata" {
        . $helperPath

        $releaseRoot = Resolve-CcswitchmultiReleaseRoot `
            -RepoRoot 'C:\not-a-repository' `
            -RequestedRoot 'D:\ccsm-release'

        $releaseRoot | Should Be 'D:\ccsm-release'
    }

    It "fails clearly when the default export root cannot be tied to Git metadata" {
        . $helperPath

        { Resolve-CcswitchmultiReleaseRoot -RepoRoot 'C:\not-a-repository' -GitCommonDir '' } |
            Should Throw 'cannot resolve the CCSwitchMulti main repository'
    }

    It "exports the projected NSIS-installed executable hash before final checksums" {
        $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
        $exportScript = [System.IO.File]::ReadAllText((Join-Path $repoRoot "scripts\export-latest-ccswitchmulti.ps1"))
        $sourceOffset = $exportScript.IndexOf('$sourceExe = Join-Path $releaseDir "cc-switch.exe"')
        $installedHashOffset = $exportScript.IndexOf('Write-NsisInstalledExeHash -SourceExe $sourceExe')
        $checksumsOffset = $exportScript.LastIndexOf('Write-Checksums -Root $exportRoot')

        $sourceOffset | Should BeGreaterThan -1
        $installedHashOffset | Should BeGreaterThan $sourceOffset
        $checksumsOffset | Should BeGreaterThan $installedHashOffset
    }

    It "writes exported text through the BOM-free UTF-8 helper" {
        $repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
        $exportScript = [System.IO.File]::ReadAllText((Join-Path $repoRoot "scripts\export-latest-ccswitchmulti.ps1"))

        $exportScript | Should Match "function Write-Utf8NoBom"
        $exportScript | Should Match 'UTF8Encoding\]::new\(\$false\)'
        $exportScript | Should Not Match "Set-Content[^\r\n]*-Encoding UTF8"
        $exportScript.Contains('Write-Utf8NoBom -Path (Join-Path $Root "latest.json")') | Should Be $true
    }
}
