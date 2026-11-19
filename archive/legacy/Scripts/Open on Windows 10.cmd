@echo off
setlocal
set "runtime=%~dp0WebView2Runtime"
icacls "%runtime%" /grant "*S-1-15-2-1:(OI)(CI)(RX)" "*S-1-15-2-2:(OI)(CI)(RX)" /Q
if errorlevel 1 (
    echo Extract the ZIP to a local folder you own and try again.
    pause
    exit /b 1
)
start "" "%~dp0QueueLauncher.exe"
