param(
    [ValidateSet('enable', 'disable', 'status')]
    [string]$Action = 'status',
    [string]$SessionId,
    [ValidateRange(1, 30)]
    [int]$Minutes = 10,
    [ValidateRange(1, 20)]
    [int]$MaxEvents = 6,
    [string]$ConfigDir = (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cc-switch')
)
$ErrorActionPreference = 'Stop'
$controlPath = Join-Path ([IO.Path]::GetFullPath($ConfigDir)) 'codex-error-capture.json'
if ($Action -eq 'disable') {
    if (Test-Path -LiteralPath $controlPath) { Remove-Item -LiteralPath $controlPath }
    Write-Output 'Capture disabled; existing diagnostic files retained.'
    return
}
if ($Action -eq 'status') {
    if (-not (Test-Path -LiteralPath $controlPath)) { Write-Output 'Capture disabled.'; return }
    $control = [IO.File]::ReadAllText($controlPath, [Text.UTF8Encoding]::new($false, $true)) | ConvertFrom-Json
    [pscustomobject]@{
        SessionId = $control.session_id
        CaptureId = $control.capture_id
        ExpiresAt = [DateTimeOffset]::FromUnixTimeSeconds($control.expires_at).ToLocalTime()
        Expired = ([DateTimeOffset]::UtcNow.ToUnixTimeSeconds() -ge $control.expires_at)
        MaxEvents = $control.max_events
        OutputDir = Join-Path $ConfigDir ('logs/codex-error-captures/' + $control.capture_id)
        RuntimeSupport = 'Requires a CCSM build containing codex_error_capture; control file alone is not runtime verification.'
    }
    return
}
$parsedSession = [Guid]::Empty
if (-not [Guid]::TryParseExact($SessionId, 'D', [ref]$parsedSession)) { throw 'SessionId must be a canonical task UUID.' }
$control = [ordered]@{
    capture_id = [Guid]::NewGuid().ToString('D')
    session_id = $parsedSession.ToString('D')
    expires_at = [DateTimeOffset]::UtcNow.AddMinutes($Minutes).ToUnixTimeSeconds()
    max_events = $MaxEvents
}
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($controlPath)) | Out-Null
[IO.File]::WriteAllText($controlPath, ($control | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
Write-Output ('Capture control enabled for ' + $control.session_id + '. Requires the diagnostic build; no service was restarted.')
Write-Output (Join-Path $ConfigDir ('logs/codex-error-captures/' + $control.capture_id))
