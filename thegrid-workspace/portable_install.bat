@echo off
:: The Grid - Portable Deployment Utility
:: Run as Administrator to unblock the app and create shortcuts.

set APP_NAME=The Grid
set EXE_NAME=thegrid.exe
set NODE_EXE_NAME=thegrid-node.exe

echo [!] Installing %APP_NAME%...

:: 1. Self-Unblock
powershell -Command "Unblock-File -Path '%~dp0%EXE_NAME%'"

:: 2. Add Exclusion for Windows Defender
echo [!] Whitelisting folder in Windows Defender...
powershell -Command "Add-MpPreference -ExclusionPath '%~dp0'"

:: 3. Register Windows Explorer right-click actions (HKCU)
echo [!] Registering Explorer context menu actions...
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install_explorer_context_menu.ps1" -Action install -ExePath "%~dp0%EXE_NAME%"

:: 4. Create Desktop Shortcut
echo [!] Creating Desktop Shortcut...
set SCRIPT_PATH=%TEMP%\create_shortcut.vbs
echo Set oWS = WScript.CreateObject("WScript.Shell") > %SCRIPT_PATH%
echo sLinkFile = oWS.SpecialFolders("Desktop") ^& "\%APP_NAME%.lnk" >> %SCRIPT_PATH%
echo Set oLink = oWS.CreateShortcut(sLinkFile) >> %SCRIPT_PATH%
echo oLink.TargetPath = "%~dp0%EXE_NAME%" >> %SCRIPT_PATH%
echo oLink.WorkingDirectory = "%~dp0" >> %SCRIPT_PATH%
echo oLink.Description = "Secure Mesh Network and AI Terminal" >> %SCRIPT_PATH%
echo oLink.Save >> %SCRIPT_PATH%

echo If CreateObject("Scripting.FileSystemObject").FileExists("%~dp0%NODE_EXE_NAME%") Then >> %SCRIPT_PATH%
echo   sNodeLinkFile = oWS.SpecialFolders("Desktop") ^& "\%APP_NAME% Node.lnk" >> %SCRIPT_PATH%
echo   Set oNodeLink = oWS.CreateShortcut(sNodeLinkFile) >> %SCRIPT_PATH%
echo   oNodeLink.TargetPath = "%~dp0%NODE_EXE_NAME%" >> %SCRIPT_PATH%
echo   oNodeLink.WorkingDirectory = "%~dp0" >> %SCRIPT_PATH%
echo   oNodeLink.Description = "The Grid headless node" >> %SCRIPT_PATH%
echo   oNodeLink.Save >> %SCRIPT_PATH%
echo End If >> %SCRIPT_PATH%
cscript //nologo %SCRIPT_PATH%
del %SCRIPT_PATH%

echo [OK] %APP_NAME% is ready! You can now launch it from your Desktop.
pause
