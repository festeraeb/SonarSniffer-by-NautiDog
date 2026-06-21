# Stage CLI sidecars for Tauri bundling (Windows).
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path,
    [string]$BuildProfile = "release"
)

$ErrorActionPreference = "Stop"
$targetDir = if ($env:CARGO_TARGET_DIR) {
    $env:CARGO_TARGET_DIR
} elseif ($env:GITHUB_ACTIONS -eq "true") {
    Join-Path $RepoRoot "target"
} else {
    Join-Path $env:LOCALAPPDATA "SonarSniffer-build\target"
}
$env:CARGO_TARGET_DIR = $targetDir

function Invoke-CargoBuild {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$BuildArgs)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    & cargo @BuildArgs
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev
    if ($code -ne 0) { throw "cargo build failed ($code)" }
}

Push-Location $RepoRoot
try {
    Write-Host "Building CLI sidecars ($BuildProfile) -> $targetDir"
    if ($BuildProfile -eq "release") {
        Invoke-CargoBuild @('build', '--release', '--no-default-features', '--bin', 'sonarsniffer-cli', '--bin', 'parse_cli')
        Invoke-CargoBuild @('build', '--release', '-p', 'soundtiles', '--bin', 'soundtiles')
    } else {
        Invoke-CargoBuild @('build', '--no-default-features', '--bin', 'sonarsniffer-cli', '--bin', 'parse_cli')
        Invoke-CargoBuild @('build', '-p', 'soundtiles', '--bin', 'soundtiles')
    }

    $binDir = Join-Path $RepoRoot "desktop\src-tauri\binaries"
    New-Item -ItemType Directory -Force -Path $binDir | Out-Null

    $ext = if ($IsWindows -or $env:OS -match "Windows") { ".exe" } else { "" }
    $outProfile = if ($BuildProfile -eq "release") { "release" } else { "debug" }
    $triple = (& rustc --print host-tuple).Trim()

    foreach ($name in @("sonarsniffer-cli", "parse_cli", "soundtiles")) {
        $src = Join-Path $targetDir "$outProfile\$name$ext"
        $dst = Join-Path $binDir "$name-$triple$ext"
        if (-not (Test-Path $src)) {
            throw "Missing build output: $src"
        }
        Copy-Item -Force $src $dst
        Write-Host "  staged $dst"
    }
}
finally {
    Pop-Location
}

Write-Host "Sidecars ready in desktop/src-tauri/binaries/"
