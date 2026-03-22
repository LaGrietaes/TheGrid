# Requires RunAs administrator
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Warning "Please run this script as an Administrator!"
    exit
}

Write-Host "Configuring LocalAccountTokenFilterPolicy to allow access to C$ administrative shares..."
New-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System" -Name "LocalAccountTokenFilterPolicy" -Value 1 -PropertyType DWord -Force -ErrorAction SilentlyContinue

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

Write-Host "Allowing inbound 5555 The Grid agent..."
Remove-NetFirewallRule -DisplayName "TheGrid Agent" -ErrorAction SilentlyContinue
New-NetFirewallRule -DisplayName "TheGrid Agent" -Direction Inbound -LocalPort 5555 -Protocol TCP -Action Allow -ErrorAction SilentlyContinue

Write-Host "Allowing ICMPv4 (Ping)..."
Enable-NetFirewallRule -Name "CoreNet-Diag-ICMP4-EchoRequest-In" -ErrorAction SilentlyContinue
Enable-NetFirewallRule -Name "FPS-ICMP4-ERQ-In" -ErrorAction SilentlyContinue

Write-Host "Configuration applied successfully!"
