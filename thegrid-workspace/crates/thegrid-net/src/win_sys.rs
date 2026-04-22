/// Windows system utilities for THE GRID agent.
/// Provides RDP enablement and query functions.

/// Returns true if Remote Desktop is currently enabled on this machine.
/// On non-Windows platforms always returns false.
pub fn is_rdp_enabled() -> bool {
    #[cfg(windows)]
    {
        use std::process::Command;
        let out = Command::new("reg")
            .args(["query", r"HKLM\System\CurrentControlSet\Control\Terminal Server", "/v", "fDenyTSConnections"])
            .output();
        match out {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                // fDenyTSConnections == 0 means RDP is allowed
                stdout.contains("0x0")
            }
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    false
}

/// Enables Remote Desktop on Windows by setting the registry key and firewall rule.
/// Returns Ok(()) on success, Err on failure or unsupported platform.
pub fn enable_rdp() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        use std::process::Command;
        let status = Command::new("reg")
            .args([
                "add",
                r"HKLM\System\CurrentControlSet\Control\Terminal Server",
                "/v", "fDenyTSConnections",
                "/t", "REG_DWORD",
                "/d", "0",
                "/f",
            ])
            .status()?;
        if !status.success() {
            return Err(anyhow::anyhow!("reg add failed"));
        }
        // Enable the firewall rule (ignore error — may already be enabled)
        let _ = Command::new("netsh")
            .args(["advfirewall", "firewall", "set", "rule",
                   "group=\"Remote Desktop\"", "new", "enable=Yes"])
            .status();
        Ok(())
    }
    #[cfg(not(windows))]
    Err(anyhow::anyhow!("RDP enablement is only supported on Windows"))
}
