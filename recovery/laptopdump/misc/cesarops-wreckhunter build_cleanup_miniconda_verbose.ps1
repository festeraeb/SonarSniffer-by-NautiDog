# CESARops Miniconda Cleanup Script - VERBOSE
# Run this AFTER uninstalling via Windows Add/Remove Programs
# Does NOT require Administrator (only cleans user-accessible locations)

$ErrorActionPreference = "Continue"

Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "MINICONDA CLEANUP SCRIPT - VERBOSE MODE" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "This script will:" -ForegroundColor White
Write-Host "  1. Delete Miniconda directories" -ForegroundColor White
Write-Host "  2. Clean PATH entries (User and System)" -ForegroundColor White
Write-Host "  3. Clean registry entries" -ForegroundColor White
Write-Host ""
Write-Host "Starting cleanup at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Green
Write-Host ""

# ============================================================================
# STEP 1: Delete Miniconda Directories
# ============================================================================
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "[STEP 1/4] Deleting Miniconda Directories" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host ""

$directoriesToDelete = @(
    "C:\Users\thomf\miniconda3",
    "C:\Users\thomf\.conda",
    "C:\Users\thomf\.condarc",
    "C:\ProgramData\miniconda3",
    "C:\ProgramData\conda",
    "$env:LOCALAPPDATA\conda",
    "$env:USERPROFILE\.conda",
    "$env:USERPROFILE\.condarc"
)

foreach ($dir in $directoriesToDelete) {
    Write-Host "Checking: $dir" -ForegroundColor Gray
    if (Test-Path $dir) {
        Write-Host "  FOUND - Attempting to delete..." -ForegroundColor Yellow
        try {
            Remove-Item -Path $dir -Recurse -Force -ErrorAction Stop
            Write-Host "  SUCCESS - Deleted" -ForegroundColor Green
        }
        catch {
            Write-Host "  FAILED - $_" -ForegroundColor Red
            Write-Host "  ACTION REQUIRED: Manually delete this folder after restart" -ForegroundColor Red
        }
    }
    else {
        Write-Host "  Not found - OK" -ForegroundColor DarkGray
    }
    Write-Host ""
}

# ============================================================================
# STEP 2: Clean User PATH
# ============================================================================
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "[STEP 2/4] Cleaning User PATH Entries" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host ""

$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
Write-Host "Current User PATH has $($userPath.Split(';').Count) entries" -ForegroundColor Gray
Write-Host ""

$paths = $userPath -split ';'
$originalCount = $paths.Count

$minicondaPatterns = @(
    "miniconda3",
    "miniconda",
    "\conda"
)

$removedPaths = @()
foreach ($path in $paths) {
    $isMiniconda = $false
    foreach ($pattern in $minicondaPatterns) {
        if ($path -like "*$pattern*") {
            $isMiniconda = $true
            break
        }
    }
    
    if ($isMiniconda) {
        Write-Host "REMOVING: $path" -ForegroundColor Yellow
        $removedPaths += $path
    }
}

# Actually remove them
foreach ($toRemove in $removedPaths) {
    $paths = $paths | Where-Object { $_ -ne $toRemove }
}

$newUserPath = $paths -join ';'
[Environment]::SetEnvironmentVariable("PATH", $newUserPath, "User")

Write-Host ""
Write-Host "Summary:" -ForegroundColor White
Write-Host "  Original entries: $originalCount" -ForegroundColor Gray
Write-Host "  Removed entries:  $($removedPaths.Count)" -ForegroundColor Yellow
Write-Host "  Remaining entries: $($paths.Count)" -ForegroundColor Green
Write-Host ""

if ($removedPaths.Count -eq 0) {
    Write-Host "No Miniconda entries found in User PATH - OK" -ForegroundColor Green
}
Write-Host ""

# ============================================================================
# STEP 3: Clean System PATH
# ============================================================================
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "[STEP 3/4] Cleaning System PATH Entries" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host ""

# Note: This reads System PATH but may not be able to write without admin
$systemPath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
Write-Host "Current System PATH has $($systemPath.Split(';').Count) entries" -ForegroundColor Gray
Write-Host ""

$paths = $systemPath -split ';'
$originalCount = $paths.Count

$removedPaths = @()
foreach ($path in $paths) {
    $isMiniconda = $false
    foreach ($pattern in $minicondaPatterns) {
        if ($path -like "*$pattern*") {
            $isMiniconda = $true
            break
        }
    }
    
    if ($isMiniconda) {
        Write-Host "FOUND (requires admin to remove): $path" -ForegroundColor Yellow
        $removedPaths += $path
    }
}

Write-Host ""
Write-Host "Summary:" -ForegroundColor White
Write-Host "  Original entries: $originalCount" -ForegroundColor Gray
Write-Host "  Miniconda entries found: $($removedPaths.Count)" -ForegroundColor Yellow
Write-Host ""

if ($removedPaths.Count -gt 0) {
    Write-Host "NOTE: System PATH cleanup requires Administrator privileges." -ForegroundColor Yellow
    Write-Host "These entries will be cleaned by the fix_path_system.ps1 script." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Miniconda entries in System PATH:" -ForegroundColor Yellow
    foreach ($path in $removedPaths) {
        Write-Host "  - $path" -ForegroundColor Yellow
    }
    Write-Host ""
}
else {
    Write-Host "No Miniconda entries found in System PATH - OK" -ForegroundColor Green
    Write-Host ""
}

# ============================================================================
# STEP 4: Clean Registry
# ============================================================================
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "[STEP 4/4] Cleaning Registry Entries" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host ""

$registryPaths = @(
    "HKCU:\Software\conda",
    "HKCU:\Software\Miniconda3",
    "HKCU:\Software\Python\conda"
)

foreach ($regPath in $registryPaths) {
    Write-Host "Checking: $regPath" -ForegroundColor Gray
    if (Test-Path $regPath) {
        Write-Host "  FOUND - Attempting to delete..." -ForegroundColor Yellow
        try {
            Remove-Item -Path $regPath -Recurse -Force -ErrorAction Stop
            Write-Host "  SUCCESS - Deleted" -ForegroundColor Green
        }
        catch {
            Write-Host "  FAILED - $_" -ForegroundColor Red
        }
    }
    else {
        Write-Host "  Not found - OK" -ForegroundColor DarkGray
    }
    Write-Host ""
}

# ============================================================================
# FINAL SUMMARY
# ============================================================================
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host "CLEANUP COMPLETE" -ForegroundColor Cyan
Write-Host "================================================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "Completed at $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Green
Write-Host ""
Write-Host "NEXT STEPS:" -ForegroundColor White
Write-Host "  1. RESTART YOUR COMPUTER (required for PATH changes to take effect)" -ForegroundColor Yellow
Write-Host "  2. After restart, test the build:" -ForegroundColor White
Write-Host "     cd c:\Users\thomf\programming\wreckhunter2000\cesarops-search" -ForegroundColor Gray
Write-Host "     cargo clean" -ForegroundColor Gray
Write-Host "     cargo build --release" -ForegroundColor Gray
Write-Host ""
Write-Host "  3. If build succeeds, reinstall Miniconda (optional):" -ForegroundColor White
Write-Host "     - Download from: https://docs.conda.io/en/latest/miniconda.html" -ForegroundColor Gray
Write-Host "     - Install to different location (e.g., C:\miniconda3)" -ForegroundColor Gray
Write-Host "     - DO NOT add to PATH during installation" -ForegroundColor Gray
Write-Host ""
Write-Host "================================================================================" -ForegroundColor Cyan
