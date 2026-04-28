[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("install", "uninstall")]
    [string]$Action,

    [Parameter(Mandatory = $false)]
    [string]$ExePath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-TheGridExePath {
    param([string]$Provided)

    if ($Provided -and $Provided.Trim().Length -gt 0) {
        return (Resolve-Path $Provided).Path
    }

    $repoRoot = Split-Path -Parent $PSScriptRoot
    $candidates = @(
        (Join-Path $repoRoot "target\release\thegrid.exe"),
        (Join-Path $repoRoot "target\debug\thegrid.exe")
    )

    foreach ($c in $candidates) {
        if (Test-Path $c) {
            return (Resolve-Path $c).Path
        }
    }

    throw "Could not find thegrid.exe. Pass -ExePath explicitly."
}

function Ensure-Key {
    param([string]$Path)
    if (-not (Test-Path $Path)) {
        New-Item -Path $Path -Force | Out-Null
    }
}

function Set-MenuCommand {
    param(
        [string]$RootKey,
        [string]$MenuName,
        [string]$MenuText,
        [string]$Command
    )

    $menuKey = "$RootKey\\shell\\$MenuName"
    $cmdKey  = "$menuKey\\command"

    Ensure-Key -Path $menuKey
    Ensure-Key -Path $cmdKey

    New-ItemProperty -Path $menuKey -Name "MUIVerb" -Value $MenuText -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $menuKey -Name "Icon" -Value $script:TheGridExe -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $cmdKey  -Name "(default)" -Value $Command -PropertyType String -Force | Out-Null
}

function Remove-MenuCommand {
    param(
        [string]$RootKey,
        [string]$MenuName
    )
    $menuKey = "$RootKey\\shell\\$MenuName"
    if (Test-Path $menuKey) {
        Remove-Item -Path $menuKey -Recurse -Force
    }
}

$baseKeys = @(
    "HKCU:\Software\Classes\Directory",
    "HKCU:\Software\Classes\Directory\Background",
    "HKCU:\Software\Classes\*"
)

if ($Action -eq "install") {
    $script:TheGridExe = Get-TheGridExePath -Provided $ExePath
    Write-Host "Using exe: $script:TheGridExe"

    # Folder right-click: scan folder
    Set-MenuCommand -RootKey "HKCU:\Software\Classes\Directory" -MenuName "TheGrid.ScanFolder" -MenuText "Scan with The Grid" -Command "`"$script:TheGridExe`" --scan `"%1`""

    # Folder right-click: open ingest mode
    Set-MenuCommand -RootKey "HKCU:\Software\Classes\Directory" -MenuName "TheGrid.IngestFolder" -MenuText "Open Media Ingest in The Grid" -Command "`"$script:TheGridExe`" --ingest `"%1`""

    # Background right-click in a folder: scan current folder
    Set-MenuCommand -RootKey "HKCU:\Software\Classes\Directory\Background" -MenuName "TheGrid.ScanHere" -MenuText "Scan this folder with The Grid" -Command "`"$script:TheGridExe`" --scan `"%V`""

    # File right-click: open file context in ingest view
    Set-MenuCommand -RootKey "HKCU:\Software\Classes\*" -MenuName "TheGrid.OpenFile" -MenuText "Process with The Grid" -Command "`"$script:TheGridExe`" --open `"%1`""

    Write-Host "Installed Explorer context menu entries under HKCU." -ForegroundColor Green
    Write-Host "Tip: restart Explorer (or sign out/in) if entries do not appear immediately."
}
else {
    Remove-MenuCommand -RootKey "HKCU:\Software\Classes\Directory" -MenuName "TheGrid.ScanFolder"
    Remove-MenuCommand -RootKey "HKCU:\Software\Classes\Directory" -MenuName "TheGrid.IngestFolder"
    Remove-MenuCommand -RootKey "HKCU:\Software\Classes\Directory\Background" -MenuName "TheGrid.ScanHere"
    Remove-MenuCommand -RootKey "HKCU:\Software\Classes\*" -MenuName "TheGrid.OpenFile"

    Write-Host "Removed The Grid Explorer context menu entries." -ForegroundColor Yellow
}
