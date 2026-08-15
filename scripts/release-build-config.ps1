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
