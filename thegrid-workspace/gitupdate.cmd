@echo off
setlocal
echo [TheGrid] Starting update... (window will stay open until complete)
echo.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\gitupdate-node.ps1" -NodeOnly %*
set EXITCODE=%ERRORLEVEL%
if %EXITCODE% NEQ 0 (
    echo.
    echo [TheGrid] Update FAILED with exit code %EXITCODE%
    pause
)
endlocal & exit /b %EXITCODE%
