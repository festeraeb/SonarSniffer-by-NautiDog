param(
    [string]$VenvPath = ".venv",
    [int]$Retries = 3
)

function Write-ErrAndExit($msg) {
    Write-Host "ERROR: $msg" -ForegroundColor Red
    exit 1
}

Write-Host "Day0: dependency installer starting..."

$LogDir = "$PSScriptRoot\logs"
if (-not (Test-Path $LogDir)) { New-Item -ItemType Directory -Path $LogDir | Out-Null }
Write-Host "Logs => $LogDir"

# Check for nvidia-smi
$hasNvidia = $false
try {
    $nvs = & nvidia-smi 2>$null
    if ($LASTEXITCODE -eq 0) { $hasNvidia = $true }
} catch {
    $hasNvidia = $false
}

if ($hasNvidia) {
    Write-Host "nvidia-smi found — GPU present"
    $nvsText = (& nvidia-smi) -join "`n"
    $cudaLine = ($nvsText -split "`n" | Where-Object { $_ -match 'CUDA Version' })[0]
    if ($cudaLine) { $cudaVersion = ($cudaLine -split ':')[-1].Trim() } else { $cudaVersion = "unknown" }
    Write-Host "Detected CUDA Version: $cudaVersion"
} else {
    Write-Host "nvidia-smi not found — continuing but CUDA installs may fail"
    $cudaVersion = "unknown"
}

# Create / activate venv
if (-not (Test-Path $VenvPath)) {
    Write-Host "Creating virtualenv at $VenvPath"
    python -m venv $VenvPath
}

Write-Host "Activating venv"
. "$PSScriptRoot\..\$VenvPath\Scripts\Activate.ps1" 2>$null

Write-Host "Upgrading pip"
python -m pip install --upgrade pip setuptools wheel | Out-Null

function Install-Pkg($pkg) {
    $logf = Join-Path $LogDir "pip_install_$($pkg -replace '[^a-zA-Z0-9_.-]','_').log"
    for ($i=1; $i -le $Retries; $i++) {
        Write-Host "Installing $pkg (attempt $i/$Retries) -> $logf"
        & pip install $pkg 2>&1 | Out-File -FilePath $logf -Encoding utf8 -Append
        if ($LASTEXITCODE -eq 0) { return $true }
        Start-Sleep -Seconds 2
    }
    return $false
}

# Decide cupy package based on detected CUDA major version
$cupyPkg = 'cupy'
if ($cudaVersion -match '11') { $cupyPkg = 'cupy-cuda11x' }
elseif ($cudaVersion -match '12') { $cupyPkg = 'cupy-cuda12x' }

$packages = @($cupyPkg, 'torch', 'rasterio', 'earthaccess', 'sentinelsat', 'requests')

foreach ($p in $packages) {
    $ok = Install-Pkg $p
    if (-not $ok) { Write-ErrAndExit "Failed to install $p after $Retries attempts" }
}

# Quick smoke test for torch and cupy
$smokeLog = Join-Path $LogDir "smoke_test.log"
$py = @'
import sys
ok = True
try:
    import torch
    print('torch OK', torch.__version__)
    print('cuda available', torch.cuda.is_available())
except Exception as e:
    print('torch import failed', e)
    ok = False
try:
    import cupy
    print('cupy OK', cupy.__version__)
except Exception as e:
    print('cupy import failed', e)
    ok = False
if not ok:
    sys.exit(2)
'@

$py | python 2>&1 | Out-File -FilePath $smokeLog -Encoding utf8

Write-Host "Dependency installation complete. Smoke test log: $smokeLog" -ForegroundColor Green
