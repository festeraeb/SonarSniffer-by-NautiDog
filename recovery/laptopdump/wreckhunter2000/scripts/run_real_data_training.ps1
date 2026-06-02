<#
Run the real-data master calibration pipeline in wreckhunter2000.
This script will:
 1) Prepare Python + Rust + MSVC environment
 2) Pull saved Bagrecovery folders and sentinel candidates
 3) Guarantee data/sentinel exists (or fail with instruction)
 4) Execute master_calibration.py
#>

$ErrorActionPreference = 'Stop'

Write-Host "=== 1) Setup MSVC environment & Rust toolchain ==="
. "$PSScriptRoot\setup_msvc_env.ps1"

Write-Host "=== 2) Pull Bagrecovery dataset and path updates ==="
python "$PSScriptRoot\pull_saved_data_and_update_paths.py"

$sentinelRoot = Join-Path $PSScriptRoot "..\data\sentinel"
if (-not (Test-Path $sentinelRoot)) {
    Write-Host "[WARN] data/sentinel missing. Attempting to generate via Bagrecovery support script..."
    $bagrecoveryScript = "c:\Users\thomf\programming\Bagrecovery\scripts\wh2k_sentinel_cpu.py"
    if (Test-Path $bagrecoveryScript) {
        Write-Host "Running Bagrecovery sentinel generator..."
        python $bagrecoveryScript --output-dir "$(Join-Path $PSScriptRoot "..\data\sentinel")"
    } else {
        Write-Host "[ERROR] No wh2k_sentinel_cpu script available at $bagrecoveryScript."
    }
}

if (-not (Test-Path $sentinelRoot)) {
    Write-Host "[ERROR] Still missing data/sentinel. Place your real Sentinel targets under data/sentinel/{target}/... and rerun."
    throw "Missing real Sentinel data";
}

Write-Host "=== 3) Run master calibration on available real data ==="
Push-Location "$PSScriptRoot\.."
python -u -m bag_processor.master_calibration
if ($LASTEXITCODE -ne 0) {
    Write-Host "Calibration run failed - inspect output/logs."
    Pop-Location
    exit $LASTEXITCODE
}
Write-Host "Calibration run succeeded. Listing outputs..."
Get-ChildItem outputs\calibration
Pop-Location

Write-Host "=== Done ==="
