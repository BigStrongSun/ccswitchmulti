. (Join-Path (Split-Path -Parent $PSScriptRoot) 'production-artifacts.ps1')

function New-ProductionManifestFixture {
    $parent = Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) '.tmp'
    [void][IO.Directory]::CreateDirectory($parent)
    $root = Join-Path $parent ('manifest-fixture-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($root)
    $raw = Join-Path $root 'raw.exe'
    $installer = Join-Path $root 'setup.exe'
    [IO.File]::WriteAllText($raw, 'MZ__TAURI_BUNDLE_TYPE_VAR_UNKfrontend')
    [IO.File]::WriteAllText($installer, 'fixture installer')
    $hash = Get-ReleaseFileSha256 -Path $raw
    $manifest = [pscustomobject]@{
        schemaVersion = 1
        source = [pscustomobject]@{ Commit = ('a' * 40); Version = '1.2.3'; TrackedWorktree = 'clean' }
        frontend = [pscustomobject]@{ embedded = $true; executableSha256 = $hash; distSha256 = ('b' * 64); assets = @(@{path='assets/app.js';sha256=('c' * 64)}) }
        artifacts = [pscustomobject]@{
            rawExecutable = [pscustomobject]@{ path = 'raw.exe'; sha256 = $hash; installedSha256 = (Get-TauriNsisInstalledExeSha256 -Path $raw) }
            installer = [pscustomobject]@{ path = 'setup.exe'; sha256 = (Get-ReleaseFileSha256 -Path $installer) }
        }
    }
    $file = Join-Path $root 'PRODUCTION-MANIFEST.json'
    Write-ProductionJson -Path $file -Value $manifest
    return [pscustomobject]@{ Root=$root; Manifest=$manifest; Path=$file; Raw=$raw; Installer=$installer; Parent=$parent }
}

function Remove-ProductionManifestFixture {
    param($Fixture)
    if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($Fixture.Root)) -ne [IO.Path]::GetFullPath($Fixture.Parent) -or
        -not [IO.Path]::GetFileName($Fixture.Root).StartsWith('manifest-fixture-')) { throw 'unsafe manifest fixture cleanup target' }
    Remove-Item -LiteralPath $Fixture.Root -Recurse -Force
}

Describe 'Production artifact manifest' {
    It 'rejects a corrupted projected hash even when its format is valid' {
        $fixture = New-ProductionManifestFixture
        try {
            $fixture.Manifest.artifacts.rawExecutable.installedSha256 = ('d' * 64)
            Write-ProductionJson -Path $fixture.Path -Value $fixture.Manifest
            { Assert-ProductionManifest -ManifestPath $fixture.Path -ExpectedSourceCommit ('a' * 40) -ExpectedVersion '1.2.3' } | Should Throw 'projected installed hash'
        } finally { Remove-ProductionManifestFixture $fixture }
    }
    It 'validates installer and raw executable against a recorded clean source' {
        $fixture = New-ProductionManifestFixture
        try {
            $result = Assert-ProductionManifest -ManifestPath $fixture.Path -ExpectedSourceCommit ('a' * 40) -ExpectedVersion '1.2.3'
            $result.InstallerPath | Should Be $fixture.Installer
            $result.RawExecutablePath | Should Be $fixture.Raw
        } finally { Remove-ProductionManifestFixture $fixture }
    }
    It 'rejects an installer swapped after the build' {
        $fixture = New-ProductionManifestFixture
        try {
            [IO.File]::WriteAllText($fixture.Installer, 'different installer')
            { Assert-ProductionManifest -ManifestPath $fixture.Path -ExpectedSourceCommit ('a' * 40) -ExpectedVersion '1.2.3' } | Should Throw 'installer hash mismatch'
        } finally { Remove-ProductionManifestFixture $fixture }
    }
    It 'rejects a raw executable swapped after the build' {
        $fixture = New-ProductionManifestFixture
        try {
            [IO.File]::WriteAllText($fixture.Raw, 'different executable')
            { Assert-ProductionManifest -ManifestPath $fixture.Path -ExpectedSourceCommit ('a' * 40) -ExpectedVersion '1.2.3' } | Should Throw 'rawExecutable hash mismatch'
        } finally { Remove-ProductionManifestFixture $fixture }
    }
    It 'rejects a stale source commit even when the version is unchanged' {
        $fixture = New-ProductionManifestFixture
        try {
            { Assert-ProductionManifest -ManifestPath $fixture.Path -ExpectedSourceCommit ('d' * 40) -ExpectedVersion '1.2.3' } | Should Throw 'source/version'
        } finally { Remove-ProductionManifestFixture $fixture }
    }
    It 'rejects evidence from dirty tracked source' {
        $fixture = New-ProductionManifestFixture
        try {
            $fixture.Manifest.source.TrackedWorktree = 'dirty'
            Write-ProductionJson -Path $fixture.Path -Value $fixture.Manifest
            { Assert-ProductionManifest -ManifestPath $fixture.Path -ExpectedSourceCommit ('a' * 40) -ExpectedVersion '1.2.3' } | Should Throw 'source/version'
        } finally { Remove-ProductionManifestFixture $fixture }
    }
    It 'rejects a manifest path traversal before reading foreign artifacts' {
        $fixture = New-ProductionManifestFixture
        try {
            $fixture.Manifest.artifacts.installer.path = '../foreign.exe'
            Write-ProductionJson -Path $fixture.Path -Value $fixture.Manifest
            { Assert-ProductionManifest -ManifestPath $fixture.Path -ExpectedSourceCommit ('a' * 40) -ExpectedVersion '1.2.3' } | Should Throw 'relative and contained'
        } finally { Remove-ProductionManifestFixture $fixture }
    }
}
