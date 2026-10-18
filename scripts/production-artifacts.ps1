# Shared production provenance checks for build/export and installation preflight.
. (Join-Path $PSScriptRoot "release-build-config.ps1")

function Resolve-ProductionArtifactPath {
    param([string]$Root, [string]$RelativePath)
    if ([string]::IsNullOrWhiteSpace($RelativePath) -or [IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath -match ':' -or @($RelativePath -split '[/\\]' | Where-Object { $_ -eq '..' -or $_ -eq '.' }).Count) {
        throw "production artifact path must be relative and contained: $RelativePath"
    }
    $rootPath = [IO.Path]::GetFullPath($Root)
    $prefix = $rootPath.TrimEnd([char[]]@('/', '\')) + [IO.Path]::DirectorySeparatorChar
    $resolved = [IO.Path]::GetFullPath((Join-Path $rootPath $RelativePath))
    if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "production artifact escapes manifest directory"
    }
    $cursor = $resolved
    while ($cursor) {
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction SilentlyContinue
        if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "production artifact has a reparse boundary: $cursor"
        }
        $cursor = [IO.Path]::GetDirectoryName($cursor)
    }
    return $resolved
}

function Invoke-ProductionAssetVerification {
    param([string]$RepoRoot, [string]$CargoTargetDir)
    $output = & node (Join-Path $PSScriptRoot "verify-production-assets.cjs") `
        --dist (Join-Path $RepoRoot "dist") --target $CargoTargetDir `
        --executable (Join-Path $CargoTargetDir "release\cc-switch.exe")
    if ($LASTEXITCODE -ne 0) { throw "production frontend verification failed" }
    return (($output | Out-String) | ConvertFrom-Json -ErrorAction Stop)
}

function Write-ProductionJson {
    param([string]$Path, [psobject]$Value)
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
}

function Assert-ProductionManifest {
    param(
        [string]$ManifestPath,
        [string]$ExpectedSourceCommit,
        [string]$ExpectedVersion,
        [scriptblock]$ReadText = { param($Path) [IO.File]::ReadAllText($Path) },
        [scriptblock]$ReadBytes = { param($Path) ,[IO.File]::ReadAllBytes($Path) },
        [scriptblock]$HashFile = { param($Path) Get-ReleaseFileSha256 -Path $Path }
    )
    if ($ExpectedSourceCommit -notmatch '^[a-fA-F0-9]{40}$') { throw "expected production source commit must be a full Git SHA" }
    $manifestRoot = Split-Path -Parent ([IO.Path]::GetFullPath($ManifestPath))
    [void](Resolve-ProductionArtifactPath -Root $manifestRoot -RelativePath ([IO.Path]::GetFileName($ManifestPath)))
    try { $manifest = (& $ReadText $ManifestPath) | ConvertFrom-Json -ErrorAction Stop }
    catch { throw "production manifest cannot be read: $($_.Exception.Message)" }
    if ($manifest.schemaVersion -ne 1 -or $manifest.source.Commit -ne $ExpectedSourceCommit -or
        $manifest.source.Version -ne $ExpectedVersion -or $manifest.source.TrackedWorktree -ne 'clean') {
        throw "production manifest source/version does not match the requested release"
    }
    if ($manifest.frontend.embedded -ne $true -or $manifest.frontend.distSha256 -notmatch '^[a-fA-F0-9]{64}$' -or
        @($manifest.frontend.assets).Count -eq 0) { throw "production manifest lacks verified frontend evidence" }
    $paths = @{}
    foreach ($name in @('rawExecutable', 'installer')) {
        $artifact = $manifest.artifacts.$name
        if ($null -eq $artifact -or $artifact.sha256 -notmatch '^[a-fA-F0-9]{64}$') {
            throw "production manifest lacks $name hash"
        }
        $paths[$name] = Resolve-ProductionArtifactPath -Root $manifestRoot -RelativePath $artifact.path
        $actualHash = & $HashFile $paths[$name]
        if (-not [string]::Equals($actualHash, $artifact.sha256, [StringComparison]::OrdinalIgnoreCase)) {
            throw "production $name hash mismatch"
        }
    }
    if ($manifest.artifacts.rawExecutable.installedSha256 -notmatch '^[a-fA-F0-9]{64}$' -or
        -not [string]::Equals($manifest.frontend.executableSha256, $manifest.artifacts.rawExecutable.sha256, [StringComparison]::OrdinalIgnoreCase)) {
        throw "production manifest executable evidence is inconsistent"
    }
    $projectedHash = Get-TauriNsisInstalledExeSha256 -Path $paths.rawExecutable -ReadBytes $ReadBytes `
        -ExpectedRawHash $manifest.artifacts.rawExecutable.sha256
    if (-not [string]::Equals($projectedHash, $manifest.artifacts.rawExecutable.installedSha256, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'production projected installed hash does not match the verified raw executable'
    }
    return [pscustomobject]@{ Manifest = $manifest; RawExecutablePath = $paths.rawExecutable; InstallerPath = $paths.installer }
}

function Write-ProductionBuildReceipt {
    param([string]$RepoRoot, [string]$CargoTargetDir, [psobject]$Identity)
    if ($Identity.TrackedWorktree -ne 'clean') { throw "production build requires clean tracked source" }
    Assert-ReleaseSourceIdentity -Expected $Identity -Actual (Get-ReleaseSourceIdentity -RepoRoot $RepoRoot)
    $proof = Invoke-ProductionAssetVerification -RepoRoot $RepoRoot -CargoTargetDir $CargoTargetDir
    $release = Join-Path $CargoTargetDir "release"
    $raw = Join-Path $release "cc-switch.exe"
    $installer = "bundle/nsis/CCSwitchMulti_$($Identity.Version)_x64-setup.exe"
    $receipt = [pscustomobject]@{
        schemaVersion = 1
        source = $Identity
        frontend = $proof
        artifacts = [pscustomobject]@{
            rawExecutable = [pscustomobject]@{ path = 'cc-switch.exe'; sha256 = (Get-ReleaseFileSha256 -Path $raw); installedSha256 = (Get-TauriNsisInstalledExeSha256 -Path $raw) }
            installer = [pscustomobject]@{ path = $installer; sha256 = (Get-ReleaseFileSha256 -Path (Join-Path $release $installer)) }
        }
    }
    Assert-ReleaseSourceIdentity -Expected $Identity -Actual (Get-ReleaseSourceIdentity -RepoRoot $RepoRoot)
    Write-ProductionJson -Path (Join-Path $release 'PRODUCTION-BUILD.json') -Value $receipt
}

function Assert-ProductionBuildReceipt {
    param([string]$RepoRoot, [string]$CargoTargetDir, [psobject]$Identity)
    $path = Join-Path $CargoTargetDir 'release\PRODUCTION-BUILD.json'
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "production build receipt is missing; build through the release entry point before SkipBuild" }
    $validated = Assert-ProductionManifest -ManifestPath $path -ExpectedSourceCommit $Identity.Commit -ExpectedVersion $Identity.Version
    Assert-ReleaseSourceIdentity -Expected $Identity -Actual $validated.Manifest.source
    $proof = Invoke-ProductionAssetVerification -RepoRoot $RepoRoot -CargoTargetDir $CargoTargetDir
    if ($proof.distSha256 -ne $validated.Manifest.frontend.distSha256) { throw "production frontend changed since the recorded build" }
    Assert-ReleaseSourceIdentity -Expected $Identity -Actual (Get-ReleaseSourceIdentity -RepoRoot $RepoRoot)
    return $validated.Manifest
}

function Write-ExportProductionManifest {
    param([string]$Root, [psobject]$BuildReceipt)
    $version = $BuildReceipt.source.Version
    $export = [pscustomobject]@{
        schemaVersion = 1
        source = $BuildReceipt.source
        frontend = $BuildReceipt.frontend
        artifacts = [pscustomobject]@{
            rawExecutable = [pscustomobject]@{ path = "windows/raw-exe/CCSwitchMulti_$($version)_x64.exe"; sha256 = $BuildReceipt.artifacts.rawExecutable.sha256; installedSha256 = $BuildReceipt.artifacts.rawExecutable.installedSha256 }
            installer = [pscustomobject]@{ path = "windows/installer/CCSwitchMulti_$($version)_x64-setup.exe"; sha256 = $BuildReceipt.artifacts.installer.sha256 }
        }
    }
    $path = Join-Path $Root 'PRODUCTION-MANIFEST.json'
    Write-ProductionJson -Path $path -Value $export
    [void](Assert-ProductionManifest -ManifestPath $path -ExpectedSourceCommit $export.source.Commit -ExpectedVersion $version)
}
