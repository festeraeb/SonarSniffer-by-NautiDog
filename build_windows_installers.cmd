@echo off
setlocal
powershell -ExecutionPolicy Bypass -File "%~dp0build_windows_installers.ps1" %*
exit /b %errorlevel%
