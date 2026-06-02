param(
    [int]$MaxRetries = 5
)

Write-Host "Day0 orchestrator starting..."

$LogDir = "$PSScriptRoot\logs"
if (-not (Test-Path $LogDir)) { New-Item -ItemType Directory -Path $LogDir | Out-Null }
Write-Host "Orchestrator logs => $LogDir"

# 1) Ensure dependencies
$installLog = Join-Path $LogDir "installer_run.log"
Write-Host "Running dependency installer (log: $installLog)"
& powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\day0_dependency_installer.ps1" *>&1 | Out-File -FilePath $installLog -Encoding utf8
if ($LASTEXITCODE -ne 0) { Write-Host "Dependency installer reported failures (see $installLog)" -ForegroundColor Yellow }

# 2) Check CUDA availability via python (log)
$cudaLog = Join-Path $LogDir "cuda_check.log"
Write-Host "Checking CUDA availability via python (log: $cudaLog)"
python - <<'PY' 2>&1 | Out-File -FilePath $cudaLog -Encoding utf8
import sys
try:
    import torch
    print('torch', torch.__version__)
    print('cuda available', torch.cuda.is_available())
    if torch.cuda.is_available():
        print('device count:', torch.cuda.device_count())
        for i in range(torch.cuda.device_count()):
            try:
                print('device', i, torch.cuda.get_device_name(i))
            except Exception as e:
                print('device name query failed', e)
except Exception as e:
    print('torch check failed', e)
    sys.exit(2)
PY
if ($LASTEXITCODE -ne 0) { Write-Host "Torch/CUDA check failed — see $cudaLog" -ForegroundColor Red }

# 3) Build Rust helper (sandbox_app)
$cargoLog = Join-Path $LogDir "cargo_build.log"
if (Test-Path "$PSScriptRoot\..\sandbox_app") {
    Write-Host "Building sandbox_app (release) -> $cargoLog"
    Push-Location "$PSScriptRoot\..\sandbox_app"
    & cargo build --release 2>&1 | Out-File -FilePath $cargoLog -Encoding utf8
    if ($LASTEXITCODE -ne 0) { Write-Host "Cargo build failed (see $cargoLog)" -ForegroundColor Red }
    Pop-Location
} else {
    Write-Host "sandbox_app not found; skipping Rust build" -ForegroundColor Yellow
}

# 4) Prepare sentinel layout using existing helper (mapping from bagfilerecovery)
$mapLog = Join-Path $LogDir "prepare_sentinel.log"
if (Test-Path "$PSScriptRoot\setup_real_sentinel_from_bagrecovery.py") {
    Write-Host "Preparing sentinel dataset from BagRecovery -> $mapLog"
    python "$PSScriptRoot\setup_real_sentinel_from_bagrecovery.py" 2>&1 | Out-File -FilePath $mapLog -Encoding utf8
    if ($LASTEXITCODE -ne 0) { Write-Host "Sentinel setup failed (see $mapLog)" -ForegroundColor Yellow }
} else {
    Write-Host "Mapping helper not found: setup_real_sentinel_from_bagrecovery.py" -ForegroundColor Yellow
}

# 5) Run calibration with retry loop until calibration_v1.json appears
$attempt = 0
while ($attempt -lt $MaxRetries) {
    $attempt++
    $runLog = Join-Path $LogDir "run_calibration_attempt_$attempt.log"
    Write-Host "Calibration attempt $attempt/$MaxRetries -> $runLog"
    python -u -m bag_processor.master_calibration 2>&1 | Out-File -FilePath $runLog -Encoding utf8
    # check for calibration_v1.json in outputs directory
    if (Test-Path "$PSScriptRoot\..\outputs\calibration\calibration_v1.json") {
        Write-Host "calibration_v1.json produced successfully" -ForegroundColor Green
        break
    }
    Write-Host "calibration_v1.json not found after attempt $attempt (see $runLog)" -ForegroundColor Yellow
    # try to repair by re-running installer
    Write-Host "Re-running dependency installer to repair missing libs"
    & powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\day0_dependency_installer.ps1" *>&1 | Out-File -FilePath $installLog -Encoding utf8 -Append
}

if (-not (Test-Path "$PSScriptRoot\..\outputs\calibration\calibration_v1.json")) {
    Write-Host "Day0 Run exhausted retries; calibration_v1.json not produced. Check logs in $LogDir" -ForegroundColor Red
    exit 2
}

Write-Host "Day0 Run completed: calibration_v1.json created. See logs in $LogDir" -ForegroundColor Green
