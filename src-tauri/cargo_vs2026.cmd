@echo off
setlocal
call "C:\Program Files\Microsoft Visual Studio\18\Community\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64 -winsdk=10.0.22621.0
if errorlevel 1 exit /b %errorlevel%
cargo %*

