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
WizardStyle=modern
PrivilegesRequired=admin
DisableProgramGroupPage=yes

[Files]
Source: "target\release\thegrid.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "target\release\thegrid-node.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "SETUP.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\The Grid"; Filename: "{app}\thegrid.exe"
Name: "{group}\The Grid Node (Headless)"; Filename: "{app}\thegrid-node.exe"
Name: "{commondesktop}\The Grid"; Filename: "{app}\thegrid.exe"

[Run]
Filename: "{app}\thegrid.exe"; Description: "Launch The Grid"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Keep user data (config/index DB) under AppData intact across uninstall/update.
