$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $PSCommandPath
$repoRoot = Resolve-Path (Join-Path $scriptDir "..") | Select-Object -ExpandProperty Path
. (Join-Path $scriptDir "production-artifacts.ps1")

Push-Location $repoRoot
try {
    Invoke-WithLocalReleaseCargoTarget `
        -RepoRoot $repoRoot `
        -Enabled $true `
        -Action {
        param($cargoTargetDir)

        Write-Host "Using isolated Cargo target for this build: $cargoTargetDir"
        & pnpm tauri build
        $buildExitCode = $LASTEXITCODE
        if ($buildExitCode -ne 0) {
            throw "tauri build failed with exit code $buildExitCode"
        }
        [void](Invoke-ProductionAssetVerification -RepoRoot $repoRoot -CargoTargetDir $cargoTargetDir)
    }
} finally {
    Pop-Location -ErrorAction SilentlyContinue
}
