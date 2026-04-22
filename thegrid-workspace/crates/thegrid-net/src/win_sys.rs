use anyhow::{Result, bail};
use std::process::Command;

/// Checks if Remote Desktop (RDP) is enabled on this Windows machine.
/// Returns true if fDenyTSConnections is 0.
pub fn is_rdp_enabled() -> bool {
    #[cfg(not(target_os = "windows"))]
    {
        return false;
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("reg")
            .args(&["query", "HKLM\\System\\CurrentControlSet\\Control\\Terminal Server", "/v", "fDenyTSConnections"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let s = String::from_utf8_lossy(&out.stdout);
                // Typical output: "    fDenyTSConnections    REG_DWORD    0x0"
                s.contains("0x0")
            }
            _ => false,
        }
    }
}

/// Attempts to enable Remote Desktop (RDP) and configure firewall rules.
/// Requires administrative privileges.
pub fn enable_rdp() -> Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        bail!("RDP enablement is only supported on Windows.");
    }

    #[cfg(target_os = "windows")]
    {
        log::info!("Attempting to enable Remote Desktop via registry...");
        
        // 1. Enable RDP connections
        let reg_status = Command::new("reg")
            .args(&[
                "add", 
                "HKLM\\System\\CurrentControlSet\\Control\\Terminal Server", 
                "/v", "fDenyTSConnections", 
                "/t", "REG_DWORD", 
                "/d", "0", 
                "/f"
            ])
            .status()?;

        if !reg_status.success() {
            bail!("Failed to update registry fDenyTSConnections. Administrative privileges may be required.");
        }

        // 2. Enable User Authentication (NLA) - recommended
        let _ = Command::new("reg")
            .args(&[
                "add", 
                "HKLM\\System\\CurrentControlSet\\Control\\Terminal Server\\WinStations\\RDP-Tcp", 
                "/v", "UserAuthentication", 
                "/t", "REG_DWORD", 
                "/d", "1", 
                "/f"
            ])
            .status();

        log::info!("Configuring firewall rules for Remote Desktop...");
        
        // 3. Enable Firewall rules - try English and Spanish localized names
        let fw_en = Command::new("netsh")
            .args(&["advfirewall", "firewall", "set", "rule", "group=\"remote desktop\"", "new", "enable=Yes"])
            .status();
            
        let fw_es = Command::new("netsh")
            .args(&["advfirewall", "firewall", "set", "rule", "group=\"escritorio remoto\"", "new", "enable=Yes"])
            .status();

        if fw_en.map(|s| s.success()).unwrap_or(false) || fw_es.map(|s| s.success()).unwrap_or(false) {
            log::info!("Successfully enabled Remote Desktop firewall rule group.");
        } else {
            log::warn!("Failed to enable firewall rule group 'remote desktop'. You may need to enable it manually.");
        }

        Ok(())
    }
}
