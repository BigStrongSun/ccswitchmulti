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
