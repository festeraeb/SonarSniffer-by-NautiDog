# WreckHunter 2000 - Clean Rust Build Environment Setup
# This script creates an isolated Python virtual environment for the Tauri/Rust build

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "WreckHunter 2000 - Environment Setup" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""

# Navigate to frontend directory
$frontendDir = Join-Path $PSScriptRoot "..\frontend"
Set-Location $frontendDir

Write-Host "Working Directory: $frontendDir" -ForegroundColor Yellow
Write-Host ""

# ── Step 1: Create Python Virtual Environment ────────────────────────────────
Write-Host "[1/5] Creating Python virtual environment..." -ForegroundColor Cyan

$venvPath = Join-Path $frontendDir ".venv_wreckhunter"

if (Test-Path $venvPath) {
    Write-Host "  Existing venv found, removing..." -ForegroundColor Yellow
    Remove-Item -Recurse -Force $venvPath
}

python -m venv $venvPath
Write-Host "  ✓ Virtual environment created at: $venvPath" -ForegroundColor Green
Write-Host ""

# ── Step 2: Activate Virtual Environment ─────────────────────────────────────
Write-Host "[2/5] Activating virtual environment..." -ForegroundColor Cyan

$activateScript = Join-Path $venvPath "Scripts\Activate.ps1"
& $activateScript

Write-Host "  ✓ Environment activated" -ForegroundColor Green
Write-Host "  Python: $(python --version)" -ForegroundColor Gray
Write-Host "  Pip: $(pip --version)" -ForegroundColor Gray
Write-Host ""

# ── Step 3: Install Python Dependencies ──────────────────────────────────────
Write-Host "[3/5] Installing Python dependencies..." -ForegroundColor Cyan

$requirements = @(
    "pyo3",
    "numpy",
    "rasterio",
    "pyproj",
    "netCDF4",
    "requests",
    "scipy"
)

foreach ($pkg in $requirements) {
    Write-Host "  Installing $pkg..." -ForegroundColor Gray
    pip install $pkg --quiet
}

Write-Host "  ✓ Dependencies installed" -ForegroundColor Green
Write-Host ""

# ── Step 4: Install Node.js Dependencies ─────────────────────────────────────
Write-Host "[4/5] Installing Node.js dependencies..." -ForegroundColor Cyan

npm install --legacy-peer-deps

Write-Host "  ✓ Node dependencies installed" -ForegroundColor Green
Write-Host ""

# ── Step 5: Verify Rust/Tauri ────────────────────────────────────────────────
Write-Host "[5/5] Verifying Rust/Tauri installation..." -ForegroundColor Cyan

$cargoVersion = cargo --version 2>&1
$rustcVersion = rustc --version 2>&1

Write-Host "  $cargoVersion" -ForegroundColor Gray
Write-Host "  $rustcVersion" -ForegroundColor Gray

if ($LASTEXITCODE -eq 0) {
    Write-Host "  ✓ Rust toolchain verified" -ForegroundColor Green
} else {
    Write-Host "  ✗ Rust not found. Install from: https://rustup.rs/" -ForegroundColor Red
    Write-Host ""
    Write-Host "Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "Environment Setup Complete!" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "To activate the environment manually:" -ForegroundColor Yellow
Write-Host "  .\.venv_wreckhunter\Scripts\Activate.ps1" -ForegroundColor White
Write-Host ""
Write-Host "To build the Tauri app:" -ForegroundColor Yellow
Write-Host "  npm run tauri build" -ForegroundColor White
Write-Host ""
Write-Host "To run in development mode:" -ForegroundColor Yellow
Write-Host "  npm run tauri dev" -ForegroundColor White
Write-Host ""
