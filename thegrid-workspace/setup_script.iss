; -- The Grid Setup Script --

[Setup]
AppName=The Grid
AppVersion=0.1.0
DefaultDirName={autopf}\TheGrid
DefaultGroupName=The Grid
UninstallDisplayIcon={app}\thegrid.exe
Compression=lzma2
SolidCompression=yes
OutputDir=..
OutputBaseFilename=TheGrid_Setup
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64

[Files]
Source: "target\release\thegrid.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\The Grid"; Filename: "{app}\thegrid.exe"
Name: "{commondesktop}\The Grid"; Filename: "{app}\thegrid.exe"

[Run]
Filename: "{app}\thegrid.exe"; Description: "Launch The Grid"; Flags: nowait postinstall skipifsilent
