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
