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

            { Get-TauriNsisInstalledExeSha256 -Path $filePath } | Should Throw
        } finally {
            [System.IO.File]::Delete($filePath)
        }
    }
}
