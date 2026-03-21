@echo off
:: The Grid - Portable Deployment Utility
:: Run as Administrator to unblock the app and create shortcuts.

set APP_NAME=The Grid
set EXE_NAME=thegrid.exe

echo [!] Installing %APP_NAME%...

:: 1. Self-Unblock
powershell -Command "Unblock-File -Path '%~dp0%EXE_NAME%'"

:: 2. Add Exclusion for Windows Defender
echo [!] Whitelisting folder in Windows Defender...
powershell -Command "Add-MpPreference -ExclusionPath '%~dp0'"

:: 3. Create Desktop Shortcut
echo [!] Creating Desktop Shortcut...
set SCRIPT_PATH=%TEMP%\create_shortcut.vbs
echo Set oWS = WScript.CreateObject("WScript.Shell") > %SCRIPT_PATH%
echo sLinkFile = oWS.SpecialFolders("Desktop") ^& "\%APP_NAME%.lnk" >> %SCRIPT_PATH%
echo Set oLink = oWS.CreateShortcut(sLinkFile) >> %SCRIPT_PATH%
echo oLink.TargetPath = "%~dp0%EXE_NAME%" >> %SCRIPT_PATH%
echo oLink.WorkingDirectory = "%~dp0" >> %SCRIPT_PATH%
echo oLink.Description = "Secure Mesh Network and AI Terminal" >> %SCRIPT_PATH%
echo oLink.Save >> %SCRIPT_PATH%
cscript //nologo %SCRIPT_PATH%
del %SCRIPT_PATH%

echo [OK] %APP_NAME% is ready! You can now launch it from your Desktop.
pause
