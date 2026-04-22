param(
    [string]$TargetBranch = "",
    [switch]$NoCheck,
    [switch]$ReturnToPrevious
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptDir
Set-Location $repoRoot

function Run-Command {
    param([string]$Command)
    Write-Host "> $Command" -ForegroundColor Cyan
    Invoke-Expression $Command
}

$initialBranch = (git rev-parse --abbrev-ref HEAD).Trim()
if ([string]::IsNullOrWhiteSpace($TargetBranch)) {
    $TargetBranch = $initialBranch
}
$pending = git status --porcelain

if ($pending) {
    Write-Host "Working tree is not clean. Commit or stash changes before running gitupdate." -ForegroundColor Yellow
    exit 1
}

Run-Command "git fetch origin $TargetBranch"

if ($initialBranch -ne $TargetBranch) {
    Run-Command "git checkout $TargetBranch"
}

Run-Command "git pull --ff-only origin $TargetBranch"

if (-not $NoCheck) {
    Run-Command "cargo check -p thegrid-node"
    Run-Command "cargo check -p thegrid-gui"
}

if ($ReturnToPrevious -and $initialBranch -ne $TargetBranch) {
    Run-Command "git checkout $initialBranch"
}

Write-Host "Update complete. Branch: $TargetBranch" -ForegroundColor Green
