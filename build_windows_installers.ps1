#Requires -Version 5.1
[CmdletBinding()]
param(
    [ValidateSet("msi", "nsis", "both")]
    [string]$Bundle = "both",
    [ValidateSet("public", "private")]
    [string]$BuildFlavor = "public",
    [switch]$SkipBundle,
    [switch]$InstallCli,
    [switch]$AllowMissingGStreamer,
    [string]$GStreamerRoot = "",
    [string]$LicenseEmail = "support@nautidogsailing.com"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Write-Step($Message) { Write-Host "`n=== $Message ===" -ForegroundColor Cyan }
function Fail($Message) { Write-Host "ERROR: $Message" -ForegroundColor Red; exit 1 }
function Pass($Message) { Write-Host "OK: $Message" -ForegroundColor Green }
function Has($Command) { [bool](Get-Command $Command -ErrorAction SilentlyContinue) }

function Resolve-GStreamerRoot([string]$Preferred) {
    $candidates = @()
    if ($Preferred) { $candidates += $Preferred }
    if ($env:GSTREAMER_1_0_ROOT_MSVC_X86_64) { $candidates += $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 }
    $candidates += (Join-Path $env:LOCALAPPDATA "Programs\gstreamer\1.0\msvc_x86_64")
    $candidates += "C:\gstreamer\1.0\msvc_x86_64"
    $candidates += "C:\Program Files\gstreamer\1.0\msvc_x86_64"

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path (Join-Path $candidate "bin"))) {
            return (Resolve-Path $candidate).Path
        }
    }
    return $null
}

function Copy-DirectoryContents([string]$Source, [string]$Destination) {
    if (Test-Path $Destination) {
        Remove-Item -Recurse -Force $Destination
    }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    Copy-Item (Join-Path $Source "*") $Destination -Recurse -Force
}

function Ensure-PkgConfigOnPath() {
    if (Has "pkg-config") {
        return
    }

    $candidates = @(
        "C:\msys64\mingw64\bin",
        (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Packages\bloodrock.pkg-config-lite_Microsoft.Winget.Source_8wekyb3d8bbwe"),
        (Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Links")
    )

    foreach ($candidate in $candidates) {
        if (-not $candidate -or -not (Test-Path $candidate)) {
            continue
        }

        $pkgConfigExe = Join-Path $candidate "pkg-config.exe"
        if (Test-Path $pkgConfigExe) {
            $env:PATH = $candidate + ";" + $env:PATH
            return
        }
    }
}

$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $root
$shortTemp = "C:\sonarsniffer-temp"
New-Item -ItemType Directory -Force -Path $shortTemp | Out-Null
$env:TEMP = $shortTemp
$env:TMP = $shortTemp
$env:SONARSNIFFER_LICENSE_EMAIL = $LicenseEmail
if ($BuildFlavor -eq "private") {
    $env:SONARSNIFFER_PRIVATE_BUILD = "1"
} else {
    Remove-Item Env:SONARSNIFFER_PRIVATE_BUILD -ErrorAction SilentlyContinue
}

if (-not (Has "cargo")) {
    Fail "cargo was not found on PATH. Install Rust from https://rustup.rs first."
}

Ensure-PkgConfigOnPath

Write-Step "Checking host target"
$hostLine = rustc -vV | Select-String "^host:"
if (-not $hostLine) {
    Fail "Unable to determine Rust host target."
}
$targetTriple = ($hostLine.ToString() -replace "host:\s*", "").Trim()
Pass "Host target: $targetTriple"

Write-Step "Checking Tauri CLI"
$tauriInstalled = $false
try {
    $tauriVersion = cargo tauri --version 2>$null
    if ($LASTEXITCODE -eq 0) {
        $tauriInstalled = $true
        Pass $tauriVersion
    }
} catch {
}

if (-not $tauriInstalled) {
    if (-not $InstallCli) {
        Fail "cargo-tauri is not installed. Re-run with -InstallCli or install with: cargo install tauri-cli --version '^2.0'"
    }
    cargo install tauri-cli --version '^2.0'
    if ($LASTEXITCODE -ne 0) {
        Fail "Failed to install tauri-cli."
    }
    Pass "Installed tauri-cli"
}

Write-Step "Building soundtiles sidecar"
cargo build --release -p soundtiles
if ($LASTEXITCODE -ne 0) {
    Fail "soundtiles release build failed."
}

$sourceExe = Join-Path $root "target\release\soundtiles.exe"
if (-not (Test-Path $sourceExe)) {
    Fail "Expected soundtiles binary was not produced: $sourceExe"
}

$sidecarDir = Join-Path $root "src-tauri\binaries"
if (-not (Test-Path $sidecarDir)) {
    New-Item -ItemType Directory -Force -Path $sidecarDir | Out-Null
}
$sidecarExe = Join-Path $sidecarDir ("soundtiles-{0}.exe" -f $targetTriple)
Copy-Item $sourceExe $sidecarExe -Force
Pass "Bundled sidecar prepared: $sidecarExe"

Write-Step "Staging GStreamer runtime"
$gstSource = Resolve-GStreamerRoot $GStreamerRoot
$gstDest = Join-Path $root "src-tauri\gstreamer"
if ($gstSource) {
    Copy-DirectoryContents $gstSource $gstDest
    $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gstSource
    $env:PATH = (Join-Path $gstSource "bin") + ";" + $env:PATH
    Pass "GStreamer staged from $gstSource"
} elseif ($AllowMissingGStreamer) {
    if (Test-Path $gstDest) {
        Remove-Item -Recurse -Force $gstDest
    }
    New-Item -ItemType Directory -Force -Path $gstDest | Out-Null
    Write-Host "WARN: GStreamer not found. Installer will build without bundled runtime and video may fall back to GIF." -ForegroundColor Yellow
} else {
    Fail "GStreamer SDK/runtime not found. Install it first or pass -AllowMissingGStreamer."
}

if ($SkipBundle) {
    Pass "SkipBundle set. Sidecar build is complete."
    exit 0
}

$bundles = if ($Bundle -eq "both") { "msi,nsis" } else { $Bundle }

Write-Step "Building SonarSniffer installers"
cargo tauri build --features video-gstreamer --bundles $bundles
if ($LASTEXITCODE -ne 0) {
    Fail "Tauri bundle build failed."
}

$bundleRoot = Join-Path $root "target\release\bundle"
$artifacts = @()
if ($Bundle -in @("msi", "both")) {
    $artifacts += Get-ChildItem (Join-Path $bundleRoot "msi") -Filter *.msi -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -notmatch "-(public|private)(-(public|private))?$" }
}
if ($Bundle -in @("nsis", "both")) {
    $artifacts += Get-ChildItem (Join-Path $bundleRoot "nsis") -Filter *.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.BaseName -notmatch "-(public|private)(-(public|private))?$" }
}

Write-Step "Installer artifacts"
if (-not $artifacts -or $artifacts.Count -eq 0) {
    Fail "No installer artifacts were found under $bundleRoot"
}

foreach ($artifact in $artifacts) {
    $renamed = Join-Path $artifact.DirectoryName (([IO.Path]::GetFileNameWithoutExtension($artifact.Name)) + "-" + $BuildFlavor + $artifact.Extension)
    Copy-Item $artifact.FullName $renamed -Force
    Write-Host $renamed -ForegroundColor White
}
