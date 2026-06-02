#Requires -Version 5.1
<#
.SYNOPSIS
    sonarsniffer installer / dependency checker for SonarSniffer / CesarOPS.
.DESCRIPTION
    Checks for Rust, MSVC build tools, and Git; builds the sonarsniffer release
    binary; optionally auto-installs missing Rust/Git; and can immediately run
    sonarsniffer on an RSD file.
.PARAMETER InstallMissing
    Automatically install missing dependencies (Rust via rustup, Git via winget).
.PARAMETER Run
    After building, immediately run sonarsniffer on the file given by -Input.
.PARAMETER Input
    Path to an RSD file to process (requires -Run).
.PARAMETER Channel
    Sonar channel (default: auto).
.PARAMETER Tiles
    Number of tiles to process (default: 20).
.PARAMETER Destination
    Copy finished sonarsniffer_overlay.exe here (e.g. C:\Tools on PATH).
.EXAMPLE
    .\install_sonarsniffer.ps1
.EXAMPLE
    .\install_sonarsniffer.ps1 -InstallMissing
.EXAMPLE
    .\install_sonarsniffer.ps1 -Run -Input "test files\515456\Holloway.RSD" -Tiles 50
#>
[CmdletBinding()]
param(
    [switch]$InstallMissing,
    [switch]$Run,
    [string]$Input       = "",
    [string]$Channel     = "auto",
    [int]   $Tiles       = 20,
    [string]$Destination = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# --- helpers ------------------------------------------------------------------
function Write-Step($m) { Write-Host "`n??? $m" -ForegroundColor Cyan }
function Pass($m)  { Write-Host "  ?  $m" -ForegroundColor Green  }
function Fail($m)  { Write-Host "  ?  $m" -ForegroundColor Red    }
function Warn($m)  { Write-Host "  ?  $m" -ForegroundColor Yellow }
function Info($m)  { Write-Host "  �  $m" -ForegroundColor Gray   }

function Has($cmd) { return [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

# --- banner -------------------------------------------------------------------
Write-Host ""
Write-Host "+--------------------------------------------------+" -ForegroundColor Magenta
Write-Host "�  sonarsniffer Installer � NautiDog / CesarOPS      �" -ForegroundColor Magenta
Write-Host "+--------------------------------------------------+" -ForegroundColor Magenta
Write-Host ""

$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$sonarsnifferDir = Join-Path $root "sonarsniffer"
$releaseExe    = Join-Path $root "target\release\sonarsniffer_overlay.exe"
Info "Workspace : $root"

# --- 1. MSVC C++ build tools --------------------------------------------------
Write-Step "1. MSVC C++ Build Tools"
$vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vsFound = $false
if (Test-Path $vsWhere) {
    $vp = & $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($vp) { Pass "VS C++ tools at $vp"; $vsFound = $true }
}
if (-not $vsFound -and (Has "link.exe")) { Pass "link.exe on PATH"; $vsFound = $true }
if (-not $vsFound) {
    Warn "MSVC linker not found."
    Info "Install Visual Studio 2022 Build Tools (C++ workload):"
    Info "  https://aka.ms/vs/17/release/vs_BuildTools.exe"
    Info "  OR: winget install Microsoft.VisualStudio.2022.BuildTools"
}

# --- 2. Rust ------------------------------------------------------------------
Write-Step "2. Rust toolchain"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

if (Has "cargo") {
    Pass (cargo --version 2>&1)
    Pass (rustc  --version 2>&1)
    rustup toolchain install stable --no-self-update 2>&1 | Out-Null
    rustup target add x86_64-pc-windows-msvc          2>&1 | Out-Null
    Pass "stable + msvc target ready"
} elseif ($InstallMissing) {
    Info "Downloading rustup-init.exe..."
    $ri = Join-Path $env:TEMP "rustup-init.exe"
    Invoke-WebRequest "https://win.rustup.rs/x86_64" -OutFile $ri -UseBasicParsing
    & $ri -y --default-toolchain stable --profile minimal
    if ($LASTEXITCODE -ne 0) { Fail "rustup-init failed (code $LASTEXITCODE)"; Read-Host "Press [Enter] to exit..."; exit 1 }
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    rustup target add x86_64-pc-windows-msvc 2>&1 | Out-Null
    Pass "Rust installed"
} else {
    Fail "Rust not found. Install from https://rustup.rs or re-run with -InstallMissing"
    Read-Host "Press [Enter] to exit..."; exit 1
}

# --- 3. Git (optional) --------------------------------------------------------
Write-Step "3. Git (optional)"
if (Has "git") {
    Pass (git --version 2>&1)
} elseif ($InstallMissing -and (Has "winget")) {
    winget install --id Git.Git --silent --accept-package-agreements --accept-source-agreements
    $env:PATH = "C:\Program Files\Git\cmd;$env:PATH"
    Pass "Git installed"
} else {
    Warn "Git not found (not required to build). https://git-scm.com/download/win"
}

# --- 4. Workspace structure ---------------------------------------------------
Write-Step "4. Workspace structure"
$required = @(
    "Cargo.toml",
    "src-tauri\\src\\bin\\sonarsniffer_overlay.rs","src-tauri\src\garmin_rsd_parser.rs",
    "src-tauri\src\mosaic\feature.rs",
    "src-tauri\src\healing_api.rs"
)
$ok = $true
foreach ($rel in $required) {
    $full = Join-Path $root $rel
    if (Test-Path $full) { Pass $rel } else { Fail "Missing: $rel"; $ok = $false }
}
if (-not $ok) { Fail "Workspace incomplete � cannot build."; Read-Host "Press [Enter] to exit..."; exit 1 }

# --- 5. Build -----------------------------------------------------------------
Write-Step "5. Building sonarsniffer --release"
Info "First build compiles all dependencies (~2 min). Subsequent builds are fast."
Write-Host ""
Push-Location $root
try {
    cargo build --release -p sonarsniffer 2>&1 | ForEach-Object {
        $line = "$_"
        if     ($line -match "^error")     { Write-Host "  $line" -ForegroundColor Red }
        elseif ($line -match "^warning")   { Write-Host "  $line" -ForegroundColor DarkYellow }
        elseif ($line -match "Compiling")  { Write-Host "  $line" -ForegroundColor DarkGray }
        elseif ($line -match "Finished")   { Write-Host "  $line" -ForegroundColor Green }
        else                               { Write-Host "  $line" }
    }
    $code = $LASTEXITCODE
} finally { Pop-Location }

if ($code -ne 0) { Fail "Build failed (exit $code)"; Read-Host "Press [Enter] to exit..."; exit 1 }
if (-not (Test-Path $releaseExe)) { Fail "Expected binary not found: $releaseExe"; Read-Host "Press [Enter] to exit..."; exit 1 }
Pass "Binary ready: $releaseExe"

# --- 6. Optional copy to Destination -----------------------------------------
if ($Destination -ne "") {
    Write-Step "6. Installing to $Destination"
    if (-not (Test-Path $Destination)) { New-Item -ItemType Directory -Path $Destination | Out-Null }
    Copy-Item $releaseExe $Destination -Force
    Pass "Copied sonarsniffer_overlay.exe ? $Destination"
}

# --- 7. Optional run ----------------------------------------------------------
if ($Run) {
    Write-Step "7. Running sonarsniffer"
    if ([string]::IsNullOrWhiteSpace($Input)) {
        Warn "-Run specified but no -Input file given."
        Info 'Usage: .\install_sonarsniffer.ps1 -Run -Input "test files\515456\Holloway.RSD"'
        Read-Host "Press [Enter] to exit..."; exit 1
    }
    $rsd = if ([IO.Path]::IsPathRooted($Input)) { $Input } else { Join-Path $root $Input }
    if (-not (Test-Path $rsd)) { Fail "RSD file not found: $rsd"; Read-Host "Press [Enter] to exit..."; exit 1 }
    Write-Host ""
    & $releaseExe --input $rsd --channel $Channel --tiles $Tiles --verbose
    if ($LASTEXITCODE -ne 0) { Fail "sonarsniffer exited $LASTEXITCODE"; Read-Host "Press [Enter] to exit..."; exit 1 }
} else {
    Write-Host ""
    Pass "Done. To run sonarsniffer:"
    Write-Host ""
    Write-Host "  $releaseExe --input <path.to.RSD>" -ForegroundColor White
    Write-Host ""
    Write-Host '  Or:  .\install_sonarsniffer.ps1 -Run -Input "test files\515456\Holloway.RSD"' -ForegroundColor DarkGray
    Write-Host ""
}










