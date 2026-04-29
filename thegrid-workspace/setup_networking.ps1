param(
    [switch]$SkipTailAuth,
    [switch]$ForceWebAuth,
    [int]$AgentPort = 5000
)

function Get-TailscaleExe {
    $cmd = Get-Command tailscale.exe -ErrorAction SilentlyContinue
    if ($cmd -and $cmd.Source) {
        return $cmd.Source
    }
    $candidates = @(
        "$Env:ProgramFiles\Tailscale\tailscale.exe",
        "$Env:ProgramFiles(x86)\Tailscale\tailscale.exe"
    )
    foreach ($path in $candidates) {
        if (Test-Path $path) {
            return $path
        }
    }
    return $null
}

function Invoke-TailscaleWebAuth {
    param(
        [Parameter(Mandatory = $true)][string]$TailscaleExe,
        [switch]$Force
    )

    try {
        $statusJson = & $TailscaleExe status --json 2>$null
        $needsAuth = $true
        if ($statusJson) {
            $status = $statusJson | ConvertFrom-Json
            if ($status.BackendState -eq "Running") {
                $needsAuth = $false
            }
        }

        if ($needsAuth -or $Force) {
            Write-Host "Tailscale requires web authentication. Opening browser auth flow..." -ForegroundColor Yellow
            $authOutput = & $TailscaleExe up 2>&1
            if ($authOutput) {
                $authText = ($authOutput | Out-String)
                Write-Host $authText
                $match = [regex]::Match($authText, 'https://\S+')
                if ($match.Success) {
                    Write-Host "Open this URL to complete auth:" -ForegroundColor Cyan
                    Write-Host $match.Value -ForegroundColor Cyan
                    Start-Process $match.Value | Out-Null
                }
            }
        } else {
            Write-Host "Tailscale already authenticated on this device." -ForegroundColor Green
        }
    }
    catch {
        Write-Warning "Could not verify Tailscale auth state: $($_.Exception.Message)"
        Write-Host "You can manually run: tailscale up" -ForegroundColor Yellow
    }
}

# Requires RunAs administrator
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Warning "Please run this script as an Administrator!"
    exit 1
}

Write-Host "Configuring LocalAccountTokenFilterPolicy to allow access to C$ administrative shares..."
New-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "LocalAccountTokenFilterPolicy" -Value 1 -PropertyType DWord -Force -ErrorAction SilentlyContinue | Out-Null

Write-Host "Enabling RDP in Registry..."
Set-ItemProperty -Path "HKLM:\System\CurrentControlSet\Control\Terminal Server" -Name "fDenyTSConnections" -Value 0 -ErrorAction SilentlyContinue
Set-ItemProperty -Path "HKLM:\System\CurrentControlSet\Control\Terminal Server\WinStations\RDP-Tcp" -Name "UserAuthentication" -Value 1 -ErrorAction SilentlyContinue

Write-Host "Starting termservice..."
Set-Service -Name termservice -StartupType Automatic -ErrorAction SilentlyContinue
Start-Service -Name termservice -ErrorAction SilentlyContinue

Write-Host "Configuring Remote Desktop Firewall rules..."
Enable-NetFirewallRule -DisplayGroup "Escritorio remoto" -ErrorAction SilentlyContinue
Enable-NetFirewallRule -DisplayGroup "Remote Desktop" -ErrorAction SilentlyContinue

Write-Host "Configuring File and Printer Sharing..."
Enable-NetFirewallRule -DisplayGroup "Compartir impresoras y archivos" -ErrorAction SilentlyContinue
Enable-NetFirewallRule -DisplayGroup "File and Printer Sharing" -ErrorAction SilentlyContinue

Write-Host "Allowing inbound $AgentPort The Grid agent..."
Remove-NetFirewallRule -DisplayName "TheGrid Agent" -ErrorAction SilentlyContinue
New-NetFirewallRule -DisplayName "TheGrid Agent" -Direction Inbound -LocalPort $AgentPort -Protocol TCP -Action Allow -ErrorAction SilentlyContinue | Out-Null

Write-Host "Allowing ICMPv4 (Ping)..."
Enable-NetFirewallRule -Name "CoreNet-Diag-ICMP4-EchoRequest-In" -ErrorAction SilentlyContinue
Enable-NetFirewallRule -Name "FPS-ICMP4-ERQ-In" -ErrorAction SilentlyContinue

if (-not $SkipTailAuth) {
    $tailscaleExe = Get-TailscaleExe
    if (-not $tailscaleExe) {
        Write-Warning "Tailscale is not installed on this device."
        Write-Host "Download: https://tailscale.com/download/windows" -ForegroundColor Yellow
    }
    else {
        Write-Host "Tailscale detected: $tailscaleExe"
        $svc = Get-Service -Name "Tailscale" -ErrorAction SilentlyContinue
        if ($svc) {
            if ($svc.Status -ne "Running") {
                Write-Host "Starting Tailscale service..."
                Start-Service -Name "Tailscale" -ErrorAction SilentlyContinue
            }
        }
        Invoke-TailscaleWebAuth -TailscaleExe $tailscaleExe -Force:$ForceWebAuth
    }
}

Write-Host "Configuration applied successfully!"
