param(
    [Parameter(Mandatory = $true)]
    [string]$TargetIp,

    [Parameter(Mandatory = $true)]
    [string]$ApiKey,

    [string]$Branch = "main",
    [string]$RepoDir = "~/TheGrid/thegrid-workspace",
    [int]$Port = 5000,
    [int]$TimeoutSec = 15,
    [switch]$StashLocalChanges,
    [switch]$RunSmoke,
    [string]$Requester = "WORKSTATION-HUB"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$baseUrl = "http://$TargetIp`:$Port"
$headers = @{ "X-Grid-Key" = $ApiKey }

function Read-TerminalChunk {
    param([string]$SessionId)
    $out = Invoke-WebRequest -UseBasicParsing -Method Get -Uri "$baseUrl/v1/terminal/output?id=$SessionId" -Headers $headers -TimeoutSec $TimeoutSec
    if (-not $out.Content) { return "" }
    return [System.Text.Encoding]::UTF8.GetString($out.Content)
}

function Invoke-RemoteTerminalCommand {
    param(
        [string]$Command,
        [string]$DoneToken = "GRID_UPGRADE_DONE",
        [int]$MaxPolls = 120
    )

    $session = Invoke-RestMethod -Method Post -Uri "$baseUrl/v1/terminal/session" -Headers $headers -TimeoutSec $TimeoutSec
    $sid = [string]$session.session_id
    if ([string]::IsNullOrWhiteSpace($sid)) {
        throw "No session_id returned by remote terminal"
    }

    # Drain banner/prompt noise before sending command.
    for ($i = 0; $i -lt 3; $i++) {
        $null = Read-TerminalChunk -SessionId $sid
    }

    $fullCommand = "$Command`necho '$DoneToken'`n"
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($fullCommand)
    Invoke-WebRequest -UseBasicParsing -Method Post -Uri "$baseUrl/v1/terminal/input?id=$sid" -Headers $headers -Body $bytes -TimeoutSec $TimeoutSec | Out-Null

    $all = ""
    for ($i = 0; $i -lt $MaxPolls; $i++) {
        $chunk = Read-TerminalChunk -SessionId $sid
        if (-not [string]::IsNullOrEmpty($chunk)) {
            $all += $chunk
            if ($all -match [Regex]::Escape($DoneToken)) {
                break
            }
        }
    }

    return $all
}

Write-Host "Upgrading headless node on $baseUrl (branch=$Branch)" -ForegroundColor Cyan

if (-not $PSBoundParameters.ContainsKey('StashLocalChanges')) {
    $StashLocalChanges = $true
}

$stashCmd = if ($StashLocalChanges) {
    "git stash push -u -m thegrid-auto-upgrade-$(Get-Date -Format yyyyMMddHHmmss) || true"
} else {
    "echo 'stash skipped'"
}

$remoteCmd = @"
cd $RepoDir
pwd
git status --short
$stashCmd
git fetch origin
git checkout $Branch
git pull --ff-only origin $Branch
cargo build --release -p thegrid-node
git rev-parse --short HEAD
"@

$output = Invoke-RemoteTerminalCommand -Command $remoteCmd
Write-Host "---- Remote upgrade output ----" -ForegroundColor DarkCyan
Write-Host $output

if ($output -match "error:|fatal:|panicked|failed") {
    throw "Remote upgrade reported an error. Check output above."
}

Write-Host "Headless upgrade completed." -ForegroundColor Green

if ($RunSmoke) {
    $smokeScript = Join-Path $PSScriptRoot "mesh_connection_smoke.ps1"
    if (-not (Test-Path $smokeScript)) {
        throw "Smoke script not found: $smokeScript"
    }

    Write-Host "Running smoke after upgrade..." -ForegroundColor Cyan
    powershell -NoProfile -ExecutionPolicy Bypass -File $smokeScript -TargetIp $TargetIp -ApiKey $ApiKey -Requester $Requester
}
