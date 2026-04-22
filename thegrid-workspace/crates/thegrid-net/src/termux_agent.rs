use anyhow::Result;
use std::process::Command;

/// Connection method priority: OTG (fastest) > WiFi/LAN > Tailscale (remote/fallback)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConnectionMethod {
    Otg = 2,           // USB-C direct (fastest, local only)
    Lan = 1,           // WiFi/Ethernet (local network)
    Tailscale = 0,     // Tailscale mesh (works remote, slower)
}

impl std::fmt::Display for ConnectionMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Otg => write!(f, "OTG (USB-C)"),
            Self::Lan => write!(f, "LAN (WiFi)"),
            Self::Tailscale => write!(f, "Tailscale"),
        }
    }
}

/// Detect and communicate with Termux (Android) nodes running TheGrid headless via ADB/USB-C OTG.
pub struct TermuxAgent {
    serial: String,
    local_port: u16,
    remote_port: u16,
    /// Preferred connection methods in order: OTG → LAN → Tailscale
    connection: ConnectionMethod,
    /// Endpoint URL (localhost:5000 for OTG, LAN IP, or Tailscale IP)
    endpoint_url: String,
}

impl TermuxAgent {
    /// Scan for connected Android device via ADB.
    pub fn detect() -> Option<Self> {
        let output = Command::new("adb")
            .arg("devices")
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.contains("device") && !line.contains("devices") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[1] == "device" {
                    return Some(Self {
                        serial: parts[0].to_string(),
                        local_port: 5000,
                        remote_port: 5000,
                        connection: ConnectionMethod::Otg,
                        endpoint_url: "http://localhost:5000".to_string(),
                    });
                }
            }
        }
        None
    }

    pub fn serial(&self) -> &str {
        &self.serial
    }

    pub fn connection_method(&self) -> ConnectionMethod {
        self.connection
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint_url
    }

    /// Setup USB-C OTG port forwarding: adb forward tcp:LOCAL -> tcp:REMOTE.
    /// More efficient than WiFi/LAN.
    pub fn enable_otg_forwarding(&mut self) -> Result<()> {
        let local = format!("tcp:{}", self.local_port);
        let remote = format!("tcp:{}", self.remote_port);
        let status = Command::new("adb")
            .arg("-s")
            .arg(&self.serial)
            .arg("forward")
            .arg(&local)
            .arg(&remote)
            .status()?;

        if status.success() {
            self.connection = ConnectionMethod::Otg;
            self.endpoint_url = format!("http://localhost:{}", self.local_port);
            log::info!(
                "[Termux] ✓ USB-C OTG forwarding enabled: {} → tablet:{}",
                self.endpoint_url,
                self.remote_port
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to setup ADB port forwarding"))
        }
    }

    /// Try to connect via LAN (WiFi/Ethernet). Requires device IP address.
    pub fn enable_lan_connection(&mut self, device_ip: &str) -> Result<()> {
        let url = format!("http://{}:{}", device_ip, self.remote_port);

        // Quick connectivity test
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()?;

        let _ = client
            .get(&url)
            .header("Connection", "close")
            .send()?;

        self.connection = ConnectionMethod::Lan;
        self.endpoint_url = url;
        log::info!(
            "[Termux] ✓ LAN connection established: {}",
            self.endpoint_url
        );
        Ok(())
    }

    /// Try to connect via Tailscale mesh network (works for remote devices).
    /// Requires Tailscale configured on both sides.
    pub fn enable_tailscale_connection(&mut self, tailscale_ip: &str) -> Result<()> {
        let url = format!("http://{}:{}", tailscale_ip, self.remote_port);

        // Quick connectivity test
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let _ = client
            .get(&url)
            .header("Connection", "close")
            .send()?;

        self.connection = ConnectionMethod::Tailscale;
        self.endpoint_url = url;
        log::info!(
            "[Termux] ✓ Tailscale connection established: {}",
            self.endpoint_url
        );
        Ok(())
    }

    /// Auto-detect LAN IP of device via ADB (requires device to be on same network).
    pub fn detect_lan_ip(&self) -> Option<String> {
        let output = Command::new("adb")
            .arg("shell")
            .arg("ip -o addr show | grep 'inet.*wlan0\\|inet.*eth0' | awk '{print $4}' | cut -d'/' -f1")
            .output()
            .ok()?;

        let ip = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if !ip.is_empty() && !ip.contains("inet") {
            log::debug!("[Termux] Detected LAN IP: {}", ip);
            Some(ip)
        } else {
            None
        }
    }

    /// Auto-detect Tailscale IP via ADB (requires Tailscale installed on device).
    pub fn detect_tailscale_ip(&self) -> Option<String> {
        let output = Command::new("adb")
            .arg("shell")
            .arg("tailscale ip -4")
            .output()
            .ok()?;

        let ip = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if !ip.is_empty() && ip.contains('.') {
            log::debug!("[Termux] Detected Tailscale IP: {}", ip);
            Some(ip)
        } else {
            None
        }
    }

    /// Try all connection methods in order: OTG → LAN → Tailscale.
    /// Returns true if at least one connection succeeded.
    pub fn establish_best_connection(&mut self) -> Result<()> {
        // Try OTG first (fastest, direct USB)
        if let Ok(()) = self.enable_otg_forwarding() {
            return Ok(());
        }
        log::debug!("[Termux] OTG forwarding not available, trying LAN...");

        // Try LAN (WiFi/Ethernet)
        if let Some(lan_ip) = self.detect_lan_ip() {
            if let Ok(()) = self.enable_lan_connection(&lan_ip) {
                return Ok(());
            }
        }
        log::debug!("[Termux] LAN connection not available, trying Tailscale...");

        // Try Tailscale (remote mesh network)
        if let Some(ts_ip) = self.detect_tailscale_ip() {
            if let Ok(()) = self.enable_tailscale_connection(&ts_ip) {
                return Ok(());
            }
        }

        Err(anyhow::anyhow!(
            "No connection method available for Termux device {}. Tried: OTG, LAN, Tailscale",
            self.serial
        ))
    }

    /// Check if TheGrid headless is running on the device (port 5000).
    pub fn is_thegrid_running(&self) -> Result<bool> {
        let output = Command::new("adb")
            .arg("shell")
            .arg("netstat -tlnp 2>/dev/null | grep ':5000 ' || netstat -tln | grep ':5000 '")
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.contains("LISTEN") || stdout.contains("5000"))
    }

    /// Get Android version.
    pub fn android_version(&self) -> Result<String> {
        let output = Command::new("adb")
            .arg("shell")
            .arg("getprop ro.build.version.release")
            .output()?;

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Query TheGrid API on device via active connection (blocking).
    pub fn query_api(&self, endpoint: &str, api_key: &str) -> Result<String> {
        let url = format!("{}{}", self.endpoint_url, endpoint);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        let resp = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", api_key))
            .send()?;

        if resp.status().is_success() {
            Ok(resp.text()?)
        } else {
            Err(anyhow::anyhow!("API returned {}", resp.status()))
        }
    }

    /// Get Termux device info (device_name, version, file_count, etc.).
    pub fn get_device_info(&self, api_key: &str) -> Result<serde_json::Value> {
        let json_str = self.query_api("/api/status", api_key)?;
        Ok(serde_json::from_str(&json_str)?)
    }

    /// List files indexed on the Termux node.
    pub fn list_files(&self, api_key: &str, limit: usize) -> Result<Vec<serde_json::Value>> {
        let endpoint = format!("/api/files?limit={}", limit);
        let json_str = self.query_api(&endpoint, api_key)?;
        let array: Vec<serde_json::Value> = serde_json::from_str(&json_str)?;
        Ok(array)
    }
}

/// Detect Termux and establish best connection (OTG → LAN → Tailscale fallback).
pub fn setup_termux_agent() -> Option<TermuxAgent> {
    match TermuxAgent::detect() {
        Some(mut agent) => {
            log::info!("[Termux] Device detected: {}", agent.serial());

            match agent.establish_best_connection() {
                Ok(()) => {
                    match agent.is_thegrid_running() {
                        Ok(true) => {
                            let ver = agent.android_version()
                                .unwrap_or_else(|_| "unknown".to_string());
                            log::info!(
                                "[Termux] ✓ Ready via {} — Android {}, TheGrid headless active",
                                agent.connection,
                                ver
                            );
                            Some(agent)
                        }
                        Ok(false) => {
                            log::warn!("[Termux] Device connected but TheGrid not running. Start with: termux-app → `thegrid start`");
                            None
                        }
                        Err(e) => {
                            log::warn!("[Termux] Could not verify TheGrid status: {}", e);
                            Some(agent) // Still return agent even if check failed
                        }
                    }
                }
                Err(e) => {
                    log::debug!("[Termux] Connection failed: {}", e);
                    None
                }
            }
        }
        None => {
            log::debug!("[Termux] No ADB device detected");
            None
        }
    }
}
