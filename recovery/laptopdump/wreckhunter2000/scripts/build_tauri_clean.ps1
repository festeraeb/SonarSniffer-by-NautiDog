# WreckHunter 2000 - Clean Tauri Build Script
# Uses isolated Python virtual environment to avoid conflicts

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "WreckHunter 2000 - Clean Tauri Build" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""

# Navigate to frontend directory
$frontendDir = Join-Path $PSScriptRoot "..\frontend"
Set-Location $frontendDir

Write-Host "Working Directory: $frontendDir" -ForegroundColor Yellow
Write-Host ""

# ── Step 1: Check/Create Virtual Environment ─────────────────────────────────
$venvPath = Join-Path $frontendDir ".venv_wreckhunter"

if (-not (Test-Path $venvPath)) {
    Write-Host "[!] Virtual environment not found. Running setup first..." -ForegroundColor Yellow
    Write-Host ""
    & "$PSScriptRoot\setup_wreckhunter_env.ps1"
}

# ── Step 2: Activate Virtual Environment ─────────────────────────────────────
Write-Host "[1/4] Activating Python virtual environment..." -ForegroundColor Cyan

$activateScript = Join-Path $venvPath "Scripts\Activate.ps1"
& $activateScript

Write-Host "  ✓ Environment activated" -ForegroundColor Green
Write-Host "  Python: $(python --version)" -ForegroundColor Gray
Write-Host ""

# ── Step 3: Verify Dependencies ──────────────────────────────────────────────
Write-Host "[2/4] Verifying dependencies..." -ForegroundColor Cyan

# Check Python packages
$requiredPackages = @("pyo3", "numpy", "rasterio", "pyproj", "netCDF4")
foreach ($pkg in $requiredPackages) {
    try {
        python -c "import $pkg" 2>$null
        if ($LASTEXITCODE -eq 0) {
            Write-Host "  ✓ $pkg" -ForegroundColor Green
        } else {
            Write-Host "  ✗ $pkg (missing)" -ForegroundColor Red
        }
    } catch {
        Write-Host "  ✗ $pkg (error)" -ForegroundColor Red
    }
}

# Check Node packages
if (Test-Path "node_modules") {
    Write-Host "  ✓ Node modules" -ForegroundColor Green
} else {
    Write-Host "  ✗ Node modules (run npm install)" -ForegroundColor Red
}

Write-Host ""

# ── Step 4: Build Tauri ──────────────────────────────────────────────────────
Write-Host "[3/4] Building Tauri application..." -ForegroundColor Cyan
Write-Host "  This may take 5-10 minutes for first build..." -ForegroundColor Gray
Write-Host ""

npm run tauri build

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    Write-Host "  ✓ Tauri build completed successfully" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "  ✗ Build failed. Check error messages above." -ForegroundColor Red
    Write-Host ""
    Write-Host "Common fixes:" -ForegroundColor Yellow
    Write-Host "  1. Ensure Rust is installed: rustup.rs" -ForegroundColor White
    Write-Host "  2. Run: npm install --legacy-peer-deps" -ForegroundColor White
    Write-Host "  3. Check .env.development has valid API keys" -ForegroundColor White
}

Write-Host ""

# ── Step 5: Show Build Output Location ───────────────────────────────────────
Write-Host "[4/4] Build output location:" -ForegroundColor Cyan
Write-Host ""

$bundlePath = Join-Path $frontendDir "src-tauri\target\release\bundle"
if (Test-Path $bundlePath) {
    Write-Host "  $bundlePath" -ForegroundColor Green
    Write-Host ""
    Write-Host "Contents:" -ForegroundColor Gray
    
    Get-ChildItem -Recurse -File $bundlePath | ForEach-Object {
        Write-Host "    $($_.FullName)" -ForegroundColor Gray
    }
} else {
    Write-Host "  Bundle directory not found" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "Build Complete!" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""
