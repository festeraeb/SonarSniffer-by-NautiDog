@echo off
setlocal
title Model Team Tool Uninstaller

set "TARGET_DIR=%LocalAppData%\ModelTeamTool"
set "DESKTOP_LNK=%UserProfile%\Desktop\Model Team Tool.lnk"

if exist "%DESKTOP_LNK%" del /F /Q "%DESKTOP_LNK%" >nul 2>&1

if exist "%TARGET_DIR%" (
  rmdir /S /Q "%TARGET_DIR%"
)

echo.
echo [OK] Model Team Tool removed from:
echo      %TARGET_DIR%
echo.
pause
exit /b 0
