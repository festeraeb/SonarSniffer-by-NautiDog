@echo off
local
title Model Team Tool Installer

for /f %%I in ('powershell -NoProfile -Command Get-Date -Format yyyyMMdd_hhmmss') do set "TS=%%I"
set "LOG_DIR=%TEMP%\ModelTeamToolInstaller"
if not exist "%LOG_DIR%" mkdir "%LOG_DIR%" >nul 2>&1
set "LOG=%LOG_DIR%\install_%TS%.log"

set "SRC=%~dp0model-team-tool.exe"
set "TARGET_DIR=%LocalAppData%\ModelTeamTool"
set "TARGET_EXE=%TARGET_DIR%\model-team-tool.exe"
set "TARGET_README=%TARGET_DIR%\README.md"
set "README_SRC=%~dp0README.md"
set "DESKTOP=%UserProfile%\Desktop"
set "LNK=%DESKTOP%\Model Team Tool.lnk"

echo [%date% %time%] Starting installer > "%LOG%"
echo [%date% %time%] Source EXE: %SRC% >> "%LOG%"
echo [%date% %time%] Target dir: %TARGET_DIR% >> "%LOG%"

echo.
echo [INFO] Installer log:
echo %LOG%
echo.

if not exist "%SRC%" (
  echo [%date% %time%] [ERROR] Source executable missing: %SRC% >> "%LOG%"
  echo [ERROR] model-team-tool.exe was not found next to this installer script.
  echo [ERROR] Full log: %LOG%
  start "" notepad "%LOG%"
  pause
  exit /b 1
)

if not exist "%TARGET_DIR%" (
  mkdir "%TARGET_DIR%" >> "%LOG%" 2>&1
  if error level 1 (
    echo [%date% %time%] [ERROR] Failed to create target directory. >> "%LOG%"
    echo [ERROR] Could not create install directory: %TARGET_DIR%
    echo [ERROR] Full log: %LOG%
    start "" notepad "%LOG%"
    pause
    exit /b 1
  )
)

copy /Y "%SRC%" "%TARGET_EXE%" >> "%LOG%" 2>&1
if error level 1 (
  echo [%date% %time%] [ERROR] Failed to copy executable. >> "%LOG%"
  echo [ERROR] Failed to copy executable to %TARGET_EXE%
  echo [ERROR] Full log: %LOG%
  start "" notepad "%LOG%"
  pause
  exit /b 1
)

if exist "%README_SRC%" (
  copy /Y "%README_SRC%" "%TARGET_README%" >> "%LOG%" 2>&1
)

powershell -NoProfile -ExecutionPolicy Bypass -Command "try { $W=New-Object -ComObject WScript.Shell; $S=$W.CreateShortcut('%LNK%'); $S.TargetPath='%TARGET_EXE%'; $S.WorkingDirectory='%TARGET_DIR%'; $S.IconLocation='%TARGET_EXE%,0'; $S.Save(); exit 0 } catch { Write-Host $_; exit 1 }" >> "%LOG%" 2>&1
if error level 1 (
  echo [%date% %time%] [WARN] Shortcut creation failed. >> "%LOG%"
  echo [WARN] Installed but desktop shortcut creation failed.
)

echo [%date% %time%] [OK] Install completed. >> "%LOG%"
echo.
echo [OK] Installed Model Team Tool to:
echo      %TARGET_DIR%
echo.
echo [OK] Log file:
echo      %LOG%
echo.
echo Run it with:
echo      "%TARGET_EXE%" --task "your task"
echo.
pause
exit /b 0
