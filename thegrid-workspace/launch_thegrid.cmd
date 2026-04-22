@echo off
setlocal
set "APPDIR=%~dp0"

if exist "%APPDIR%gitupdate.cmd" (
    call "%APPDIR%gitupdate.cmd" -NoCheck >nul 2>nul
)

start "" "%APPDIR%thegrid.exe"
endlocal
