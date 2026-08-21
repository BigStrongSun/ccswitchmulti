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
}
