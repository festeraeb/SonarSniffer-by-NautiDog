@echo off
setlocal
title Model Team Tool Installer (With Logs)

set "INSTALLER=%~dp0ModelTeamTool-Installer.exe"
set "LOG_DIR=%TEMP%\ModelTeamToolInstaller"

if not exist "%INSTALLER%" (
  echo [ERROR] Installer not found: %INSTALLER%
  pause
  exit /b 1
)

echo [INFO] Running installer...
start "" /wait "%INSTALLER%"

set "LATEST_LOG="
for /f "delims=" %%F in ('dir /b /o-d "%LOG_DIR%\install_*.log" 2^>nul') do (
  set "LATEST_LOG=%LOG_DIR%\%%F"
  goto :found
)

:found
if not defined LATEST_LOG (
  echo [WARN] No install log found in %LOG_DIR%
  pause
  exit /b 0
)

echo.
echo [INFO] Latest install log:
echo %LATEST_LOG%
echo.
echo ==================== INSTALL LOG START ====================
type "%LATEST_LOG%"
echo ===================== INSTALL LOG END =====================
echo.
echo Copy the log above and paste it into chat if install still fails.
pause
exit /b 0
