# Fail if library sources were copied back into the desktop crate.
param(
    [string]$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
)

$desktopSrc = Join-Path $RepoRoot "desktop/src-tauri/src"
$allowed = @("lib.rs", "main.rs", "commands.rs")

$errors = @()
if (-not (Test-Path $desktopSrc)) {
    Write-Error "Missing desktop src: $desktopSrc"
    exit 1
}

Get-ChildItem -Path $desktopSrc -Recurse -File | ForEach-Object {
    $rel = $_.FullName.Substring($desktopSrc.Length).TrimStart("\", "/")
    if ($rel -match "[\\/]") {
        $errors += "Unexpected subdirectory file: $rel"
        return
    }
    if ($allowed -notcontains $_.Name) {
        $errors += "Unexpected desktop source file: $($_.Name) (library code belongs in src/ only)"
    }
}

foreach ($name in $allowed) {
    if (-not (Test-Path (Join-Path $desktopSrc $name))) {
        $errors += "Missing required desktop file: $name"
    }
}

if ($errors.Count -gt 0) {
    Write-Host "SINGLE-CRATE CHECK FAILED" -ForegroundColor Red
    $errors | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host ""
    Write-Host "Fix: edit R:\sonarsniffer\src\ only. Remove duplicated files from desktop\src-tauri\src\."
    exit 1
}

Write-Host "OK: desktop crate is a thin Tauri shell ($($allowed -join ', '))." -ForegroundColor Green
exit 0
