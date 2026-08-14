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
