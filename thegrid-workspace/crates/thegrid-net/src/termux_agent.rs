use anyhow::Result;
use std::process::Command;

/// Detect and communicate with Termux (Android) nodes running TheGrid headless via ADB/USB-C OTG.
pub struct TermuxAgent {
    serial: String,
    local_port: u16,
    remote_port: u16,
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
                    });
                }
            }
        }
        None
    }

    /// Get detected device serial number.
    pub fn serial(&self) -> &str {
        &self.serial
    }

    /// Setup USB-C OTG port forwarding: adb forward tcp:LOCAL -> tcp:REMOTE.
    /// More efficient than WiFi/LAN.
    pub fn enable_otg_forwarding(&self) -> Result<()> {
        let cmd = format!("tcp:{}:tcp:{}", self.local_port, self.remote_port);
        let status = Command::new("adb")
            .arg("forward")
            .arg(&cmd)
            .status()?;

        if status.success() {
            log::info!("[Termux] USB-C OTG forwarding enabled: local:{} <- device:{}", 
                      self.local_port, self.remote_port);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to setup ADB port forwarding"))
        }
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

    /// Query TheGrid API on device via forwarded localhost port (blocking).
    /// Returns API response as JSON string.
    pub fn query_api(&self, endpoint: &str, api_key: &str) -> Result<String> {
        let url = format!("http://localhost:{}{}", self.local_port, endpoint);
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

/// Detect Termux and enable OTG forwarding automatically.
pub fn setup_termux_otg() -> Result<()> {
    match TermuxAgent::detect() {
        Some(agent) => {
            log::info!("[Termux] Device detected: {}", agent.serial());
            agent.enable_otg_forwarding()?;
            match agent.is_thegrid_running() {
                Ok(true) => {
                    let ver = agent.android_version().unwrap_or_else(|_| "unknown".to_string());
                    log::info!("[Termux] TheGrid headless running on Android {}", ver);
                    Ok(())
                }
                Ok(false) => {
                    Err(anyhow::anyhow!("TheGrid not running on Termux device. Start it with: termux-app -> thegrid start"))
                }
                Err(e) => {
                    log::warn!("[Termux] Could not verify TheGrid status: {}", e);
                    Ok(())
                }
            }
        }
        None => {
            log::debug!("[Termux] No ADB device detected. Continuing without Termux support.");
            Ok(())
        }
    }
}

