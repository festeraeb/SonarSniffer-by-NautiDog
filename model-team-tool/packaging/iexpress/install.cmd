@echo off
setlocal

for /f %%I in ('powershell -NoProfile -Command Get-Date -Format yyyyMMdd_HHmmss') do set "TS=%%I"
set "LOG_DIR=%TEMP%\ModelTeamToolInstaller"
if not exist "%LOG_DIR%" mkdir "%LOG_DIR%" >nul 2>&1
set "LOG=%LOG_DIR%\install_%TS%.log"

set "TARGET=%LocalAppData%\ModelTeamTool"
set "EXE_SRC=%~dp0model-team-tool.exe"
set "README_SRC=%~dp0README.md"
set "EXE_DST=%TARGET%\model-team-tool.exe"
set "README_DST=%TARGET%\README.md"
set "DESKTOP=%UserProfile%\Desktop"
set "LNK=%DESKTOP%\Model Team Tool.lnk"

echo [%date% %time%] Starting packaged installer > "%LOG%"

echo Installer log: %LOG%

if not exist "%TARGET%" (
  mkdir "%TARGET%" >> "%LOG%" 2>&1
)

copy /Y "%EXE_SRC%" "%EXE_DST%" >> "%LOG%" 2>&1
if errorlevel 1 (
  echo [%date% %time%] [ERROR] Could not copy executable. >> "%LOG%"
  start "" notepad "%LOG%"
  exit /b 1
)

if exist "%README_SRC%" (
  copy /Y "%README_SRC%" "%README_DST%" >> "%LOG%" 2>&1
)

powershell -NoProfile -ExecutionPolicy Bypass -Command "try { $W=New-Object -ComObject WScript.Shell; $S=$W.CreateShortcut('%LNK%'); $S.TargetPath='%EXE_DST%'; $S.WorkingDirectory='%TARGET%'; $S.IconLocation='%EXE_DST%,0'; $S.Save(); exit 0 } catch { Write-Host $_; exit 1 }" >> "%LOG%" 2>&1
if errorlevel 1 (
  echo [%date% %time%] [WARN] Shortcut creation failed. >> "%LOG%"
)

echo [%date% %time%] [OK] Installed to %TARGET% >> "%LOG%"
echo Installed to %TARGET%
echo Log file: %LOG%
endlocal
exit /b 0
