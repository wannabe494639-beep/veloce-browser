@echo off
setlocal
start /wait "Veloce Browser Setup" msiexec.exe /i "%~dp0veloce.msi"
set "INSTALL_RESULT=%ERRORLEVEL%"
exit /b %INSTALL_RESULT%
