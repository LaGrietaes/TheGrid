Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    Write-Host "[1/4] Checking workspace..." -ForegroundColor Cyan
    cargo check --workspace | Out-Host

    Write-Host "[2/4] Building release binaries..." -ForegroundColor Cyan
    cargo build --release -p thegrid-gui | Out-Host
    cargo build --release -p thegrid-node | Out-Host

    $iscc = Get-Command iscc -ErrorAction SilentlyContinue
    if (-not $iscc) {
        throw "Inno Setup Compiler (iscc) not found in PATH. Install Inno Setup and ensure iscc.exe is available."
    }

    Write-Host "[3/4] Compiling installer..." -ForegroundColor Cyan
    & $iscc.Source "setup_script.iss" | Out-Host

    $installerPath = Join-Path $repoRoot "TheGrid_Setup.exe"
    if (-not (Test-Path $installerPath)) {
        throw "Installer output not found at $installerPath"
    }

    $stamp = Get-Date -Format "yyyyMMdd-HHmm"
    $stampedPath = Join-Path $repoRoot ("TheGrid_Setup_" + $stamp + ".exe")
    Copy-Item -Path $installerPath -Destination $stampedPath -Force

    Write-Host "[4/4] Done." -ForegroundColor Green
    Write-Host "Installer: $installerPath"
    Write-Host "Stamped : $stampedPath"
}
finally {
    Pop-Location
}
