# Set up MSVC/C++ build environment for wreckhunter2000 in PowerShell
# Usage: dot-source this script from your active shell to keep environment variables:
#   . .\scripts\setup_msvc_env.ps1
# Then run the build command you need (cargo build, cmake, etc.).

function Get-VsWherePath {
    $candidates = @(
        "$Env:ProgramFiles\Microsoft Visual Studio\Installer\vswhere.exe",
        "$Env:ProgramFiles(x86)\Microsoft Visual Studio\Installer\vswhere.exe",
        "$Env:ProgramFiles(x86)\Microsoft Visual Studio\Common7\IDE\vswhere.exe"
    )
    foreach ($p in $candidates) {
        if (Test-Path $p) { return $p }
    }
    return $null
}

$vswhere = Get-VsWherePath
if (-not $vswhere) {
    Write-Error "vswhere.exe not found. Install Visual Studio 2022/2019 Build Tools with C++ workload."
    return
}

Write-Host "Found vswhere: $vswhere"
$vsInstallPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsInstallPath) {
    Write-Error "No Visual Studio installation with C++ build tools found. Please install the workload."
    return
}
Write-Host "Using VS install path: $vsInstallPath"

# Prefer x64 host tools
$devCmd = Join-Path $vsInstallPath 'Common7\Tools\VsDevCmd.bat'
if (-not (Test-Path $devCmd)) {
    Write-Error "VsDevCmd.bat not found under $vsInstallPath\Common7\Tools."
    return
}

Write-Host "Running: $devCmd -arch=amd64 -host_arch=x64"
cmd /c "`"$devCmd`" -arch=amd64 -host_arch=x64 && set" | ForEach-Object {
    if ($_ -match '^(PATH|INCLUDE|LIB|LIBPATH|VSCMD_ARG_HOST_ARCH|VSCMD_ARG_TGT_ARCH)=') {
        $k, $v = $_ -split('=',2)
        Set-Item -Path "Env:$k" -Value $v
    }
}

Write-Host "MSVC environment configured."
Write-Host "cl.exe => $(Get-Command cl.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Definition)" 2>$null
Write-Host "link.exe => $(Get-Command link.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Definition)" 2>$null
Write-Host "Now run: cargo build --release"