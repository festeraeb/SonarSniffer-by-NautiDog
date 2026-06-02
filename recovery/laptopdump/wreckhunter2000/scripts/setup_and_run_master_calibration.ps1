# PowerShell script to set up dependencies for Rust + CUDA + Python and run the calibration pipeline.
# Run this as Administrator (for VS Build Tools install) if needed.

$ErrorActionPreference = 'Stop'

Write-Host "=== 1) Ensure Python venv ==="
$venv = Join-Path $PSScriptRoot "..\.venv_wreckhunter"
if (-not (Test-Path $venv)) {
    python -m venv $venv
}
& "$venv\Scripts\Activate.ps1"

Write-Host "Upgrading pip and core packages..."
python -m pip install -U pip setuptools wheel
python -m pip install -U meson-python cmake

Write-Host "Installing scientific stack..."
python -m pip install numpy==2.2.4 scipy pandas rasterio pyproj pillow matplotlib

Write-Host "Installing PyTorch for CUDA..."
# ues latest cuda 11.8 for Quadro M2200 compatibility
python -m pip install --index-url https://download.pytorch.org/whl/cu118 torch torchvision torchaudio || Write-Host "Unable to install torch here; check CUDA/driver compatibility."

Write-Host "Installing extras for rust bridge"
python -m pip install requests ndarray-npy==0.8 serde_json

Write-Host "=== 2) Ensure Rust toolchain is installed ==="
if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    Write-Host "Rustup not found, installing rustup..."; iex ((New-Object System.Net.WebClient).DownloadString('https://sh.rustup.rs'))
}
rustup toolchain install stable
rustup target add x86_64-pc-windows-msvc

Write-Host "=== 3) Fix MSVC/SDK environment if needed ==="
if (-not (Test-Path "C:\Program Files (x86)\Windows Kits\10\Lib\")) {
    Write-Host "Windows SDK missing: please install Visual Studio Build Tools (C++ workload) and Windows 10/11 SDK.";
} else {
    Write-Host "Windows SDK appears present.";
}

Write-Host "=== 4) Build Rust sandbox_app ==="
Push-Location "$PSScriptRoot\..\sandbox_app"
cargo clean; cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "Rust build failed. Check Visual Studio + SDK installation.";
    Pop-Location; return
}
Pop-Location

Write-Host "=== 5) Pull saved data and attempt real sentinel data copy ==="
python "$PSScriptRoot\pull_saved_data_and_update_paths.py"

# if no real sentinel data, fallback to sample data (explicit no-synthetic can be bypassed)
if (-not (Test-Path "$PSScriptRoot\..\data\sentinel")) {
    Write-Host "[WARN] No sentinel data found in wreckhunter2000/data/sentinel; creating sample data as fallback.";
    python "$PSScriptRoot\create_sample_sentinel_data.py"
} else {
    Write-Host "[INFO] Real sentinel data found at data/sentinel, skipping synthetic generation."
}

Write-Host "=== 6) Run calibration pipeline ==="
Push-Location "$PSScriptRoot\.."
python -u -m bag_processor.master_calibration
if ($LASTEXITCODE -ne 0) {
    Write-Host "Calibration run failed. Check output/logs."
} else {
    Write-Host "Calibration run succeeded.";
    Get-ChildItem outputs\calibration
}
Pop-Location

Write-Host "=== Done ==="
