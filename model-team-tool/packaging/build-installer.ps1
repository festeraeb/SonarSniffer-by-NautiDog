param(
  [string]$Version = "0.1.0"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
  cargo build --release

  $iscc = Get-Command iscc -ErrorAction SilentlyContinue
  if (-not $iscc) {
    throw "Inno Setup Compiler (iscc) not found. Install Inno Setup and rerun this script."
  }

  & $iscc.Source ".\packaging\model-team-tool.iss" "/DAppVersion=$Version"
  Write-Host "Installer created in .\dist"
}
finally {
  Pop-Location
}
