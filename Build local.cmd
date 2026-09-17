@echo off
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-local.ps1" %*
set "build_exit=%ERRORLEVEL%"
pause
exit /b %build_exit%
