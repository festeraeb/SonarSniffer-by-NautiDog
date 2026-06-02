# CESARops Miniconda Complete Uninstall Script
# Requires Administrator privileges

Write-Host "================================================================================"
Write-Host "MINICONDA COMPLETE UNINSTALL - Administrator Required"
Write-Host "================================================================================"
Write-Host ""

# Check if running as admin
$isAdmin = ([Security.Principal.WindowsPrincipal] `
    [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole( `
    [Security.Principal.WindowsBuiltInRole]::Administrator)

if (-not $isAdmin) {
    Write-Host "ERROR: This script must be run as Administrator!" -ForegroundColor Red
    Write-Host ""
    Write-Host "Right-click PowerShell and select 'Run as Administrator', then run:"
    Write-Host "  .\uninstall_miniconda.ps1"
    Write-Host ""
    exit 1
}

Write-Host "[1/6] Stopping any running conda/python processes..." -ForegroundColor Cyan
Get-Process python -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "*miniconda*" } | Stop-Process -Force -ErrorAction SilentlyContinue
Get-Process conda -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

Write-Host "[2/6] Uninstalling Miniconda via Windows Installer..." -ForegroundColor Cyan
$uninstallKeys = Get-ChildItem "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall" -ErrorAction SilentlyContinue
$uninstallKeys += Get-ChildItem "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall" -ErrorAction SilentlyContinue

foreach ($key in $uninstallKeys) {
    $displayName = $key.GetValue("DisplayName")
    if ($displayName -like "*Miniconda*") {
        $uninstallString = $key.GetValue("UninstallString")
        if ($uninstallString) {
            Write-Host "  Found: $displayName" -ForegroundColor Yellow
            Write-Host "  Running uninstaller..." -ForegroundColor Yellow
            $uninstallString = $uninstallString.Replace('/I', '/X ').Replace('/modify', '/uninstall')
            Start-Process msiexec.exe -ArgumentList "/X $($key.PSChildName) /quiet /norestart" -Wait
            Write-Host "  Uninstalled: $displayName" -ForegroundColor Green
        }
    }
}

Write-Host "[3/6] Removing Miniconda directories..." -ForegroundColor Cyan
$minicondaPaths = @(
    "C:\Users\thomf\miniconda3",
    "C:\ProgramData\miniconda3",
    "C:\Program Files\miniconda3",
    "$env:LOCALAPPDATA\conda",
    "$env:USERPROFILE\.conda",
    "$env:USERPROFILE\.condarc"
)

foreach ($path in $minicondaPaths) {
    if (Test-Path $path) {
        Write-Host "  Removing: $path" -ForegroundColor Yellow
        Remove-Item -Path $path -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "[4/6] Cleaning SYSTEM PATH entries..." -ForegroundColor Cyan
$systemPath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
$paths = $systemPath -split ';'
$originalCount = $paths.Count

$toRemove = @(
    "C:\Users\thomf\miniconda3",
    "C:\Users\thomf\miniconda3\Library\mingw-w64\bin",
    "C:\Users\thomf\miniconda3\Library\usr\bin",
    "C:\Users\thomf\miniconda3\Library\bin",
    "C:\Users\thomf\miniconda3\Scripts",
    "C:\Users\thomf\miniconda3\bin",
    "C:\Users\thomf\miniconda3\condabin"
)

foreach ($remove in $toRemove) {
    if ($paths -contains $remove) {
        Write-Host "  Removing from SYSTEM PATH: $remove" -ForegroundColor Yellow
        $paths = $paths | Where-Object { $_ -ne $remove }
    }
}

$newSystemPath = $paths -join ';'
[Environment]::SetEnvironmentVariable("PATH", $newSystemPath, "Machine")
Write-Host "  Removed $($originalCount - $paths.Count) entries from SYSTEM PATH" -ForegroundColor Green

Write-Host "[5/6] Cleaning User PATH entries..." -ForegroundColor Cyan
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
$paths = $userPath -split ';'
$originalCount = $paths.Count

foreach ($remove in $toRemove) {
    if ($paths -contains $remove) {
        Write-Host "  Removing from User PATH: $remove" -ForegroundColor Yellow
        $paths = $paths | Where-Object { $_ -ne $remove }
    }
}

$newUserPath = $paths -join ';'
[Environment]::SetEnvironmentVariable("PATH", $newUserPath, "User")
Write-Host "  Removed $($originalCount - $paths.Count) entries from User PATH" -ForegroundColor Green

Write-Host "[6/6] Cleaning registry entries..." -ForegroundColor Cyan
$condaRegistryPaths = @(
    "HKCU:\Software\conda",
    "HKLM:\Software\conda",
    "HKCU:\Software\Miniconda3",
    "HKLM:\Software\Miniconda3"
)

foreach ($regPath in $condaRegistryPaths) {
    if (Test-Path $regPath) {
        Write-Host "  Removing: $regPath" -ForegroundColor Yellow
        Remove-Item -Path $regPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "================================================================================"
Write-Host "MINICONDA UNINSTALL COMPLETE"
Write-Host "================================================================================"
Write-Host ""
Write-Host "IMPORTANT: You MUST RESTART YOUR COMPUTER for all changes to take effect."
Write-Host ""
Write-Host "After restart:"
Write-Host "  1. Download the latest Miniconda from: https://docs.conda.io/en/latest/miniconda.html"
Write-Host "  2. Install to: C:\Users\thomf\miniconda3 (or different location)"
Write-Host "  3. DO NOT add to PATH during installation (we'll configure manually)"
Write-Host ""
Write-Host "================================================================================"
