param(
    [string]$TargetBranch = "",
    [switch]$NoBuild,
    [switch]$NodeOnly,
    [switch]$ReturnToPrevious
)

$ErrorActionPreference = "Continue"

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot  = Split-Path -Parent $scriptDir
Set-Location $repoRoot

$logFile = Join-Path $repoRoot "gitupdate.log"

function Step {
    param([string]$Msg)
    $ts = (Get-Date).ToString("HH:mm:ss")
    $line = "[$ts] $Msg"
    Write-Host $line -ForegroundColor Cyan
    Add-Content -Path $logFile -Value $line
}

function Run {
    param([string[]]$Cmd)
    $display = $Cmd -join " "
    Step ">> $display"
    & $Cmd[0] $Cmd[1..($Cmd.Count-1)]
    if ($LASTEXITCODE -ne 0) {
        $errLine = "    ERROR: exited with code $LASTEXITCODE"
        Write-Host $errLine -ForegroundColor Red
        Add-Content -Path $logFile -Value $errLine
        return $false
    }
    return $true
}

# ── Header ──────────────────────────────────────────────────────────────────
$sep = "=" * 60
Add-Content -Path $logFile -Value ""
Add-Content -Path $logFile -Value $sep
Step "TheGrid gitupdate started"
Step "Repo: $repoRoot"

# ── Branch resolution ────────────────────────────────────────────────────────
$initialBranch = (& git rev-parse --abbrev-ref HEAD 2>&1).Trim()
if ([string]::IsNullOrWhiteSpace($TargetBranch)) { $TargetBranch = $initialBranch }
Step "Branch: $initialBranch -> $TargetBranch"

# ── Dirty check ──────────────────────────────────────────────────────────────
$pending = & git status --porcelain 2>&1
if ($pending) {
    Step "Working tree has local changes — stashing before update"
    $stashed = Run git, "stash"
} else {
    $stashed = $false
}

# ── Git fetch + pull ─────────────────────────────────────────────────────────
Step "Fetching from origin..."
$ok = Run git, "fetch", "--progress", "origin", $TargetBranch
if (-not $ok) { Read-Host "Press Enter to close"; exit 1 }

if ($initialBranch -ne $TargetBranch) {
    $ok = Run git, "checkout", $TargetBranch
    if (-not $ok) { Read-Host "Press Enter to close"; exit 1 }
}

Step "Pulling latest commits..."
$pullOut = & git pull --ff-only origin $TargetBranch 2>&1
Write-Host $pullOut
Add-Content -Path $logFile -Value $pullOut

# ── Cargo build ──────────────────────────────────────────────────────────────
if (-not $NoBuild) {
    if ($NodeOnly) {
        Step "Building thegrid-node (release)... this may take a few minutes"
        $ok = Run cargo, "build", "--release", "-p", "thegrid-node"
        if (-not $ok) { Read-Host "Press Enter to close"; exit 1 }
    } else {
        Step "Building thegrid-node (release)... this may take a few minutes"
        $ok = Run cargo, "build", "--release", "-p", "thegrid-node"
        if (-not $ok) { Read-Host "Press Enter to close"; exit 1 }

        Step "Building thegrid-gui (release)... this may take a few minutes"
        $ok = Run cargo, "build", "--release", "-p", "thegrid-gui"
        if (-not $ok) { Read-Host "Press Enter to close"; exit 1 }
    }
} else {
    Step "Skipping build (-NoBuild flag set)"
}

# ── Pop stash if we stashed ───────────────────────────────────────────────────
if ($stashed) {
    Step "Restoring stashed local changes"
    $null = Run git, "stash", "pop"
}

# ── Return to previous branch ─────────────────────────────────────────────────
if ($ReturnToPrevious -and $initialBranch -ne $TargetBranch) {
    $null = Run git, "checkout", $initialBranch
}

Step "All done. Branch: $TargetBranch"
Write-Host ""
Write-Host "Log saved to: $logFile" -ForegroundColor DarkGray
Write-Host ""
Write-Host "Press Enter to close..." -ForegroundColor Yellow
$null = Read-Host
