/// Termux (Android) agent discovery and connection management.

use anyhow::Result;
use std::fmt;

/// The transport method used to reach the Termux/Android node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionMethod {
    UsbOtg,
    LocalLan,
    Tailscale,
}

impl fmt::Display for ConnectionMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionMethod::UsbOtg   => write!(f, "USB-OTG"),
            ConnectionMethod::LocalLan  => write!(f, "LAN"),
            ConnectionMethod::Tailscale => write!(f, "Tailscale"),
        }
    }
}

/// Represents a connected Termux/Android agent node.
pub struct TermuxAgent {
    endpoint: String,
    method:   ConnectionMethod,
}

impl TermuxAgent {
    pub fn new(endpoint: String, method: ConnectionMethod) -> Self {
        Self { endpoint, method }
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn connection_method(&self) -> &ConnectionMethod {
        &self.method
    }
    pub fn establish_best_connection(&mut self) -> Result<()> {
        if let Some((ep, m)) = probe_connection_methods() {
            self.endpoint = ep;
            self.method   = m;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No Termux agent reachable via any transport"))
        }
    }
}

/// Try to find and connect to a Termux agent automatically.
pub fn setup_termux_agent() -> Option<TermuxAgent> {
    probe_connection_methods().map(|(ep, m)| TermuxAgent::new(ep, m))
}

const TERMUX_AGENT_PORT: u16 = 8765;

fn probe_connection_methods() -> Option<(String, ConnectionMethod)> {
    if let Some(ep) = probe_otg() {
        return Some((ep, ConnectionMethod::UsbOtg));
    }
    None
}

fn probe_otg() -> Option<String> {
    let out = std::process::Command::new("adb")
        .args(["devices"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let connected = stdout
        .lines()
        .skip(1)
        .any(|l| l.contains("device") && !l.is_empty());

    if !connected {
        return None;
    }

    let _ = std::process::Command::new("adb")
        .args(["forward", &format!("tcp:{}", TERMUX_AGENT_PORT), &format!("tcp:{}", TERMUX_AGENT_PORT)])
        .status()
        .ok()?;

    let ep = format!("127.0.0.1:{}", TERMUX_AGENT_PORT);
    if http_ping(&ep) { Some(ep) } else { None }
}

fn http_ping(endpoint: &str) -> bool {
    let url = format!("http://{}/ping", endpoint);
    match reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(300))
        .build()
        .ok()
        .and_then(|c| c.get(&url).send().ok())
    {
        Some(r) => r.status().is_success(),
        None    => false,
    }
}
