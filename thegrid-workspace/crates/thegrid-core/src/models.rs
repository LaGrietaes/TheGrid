use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

/// A device on the user's Tailscale mesh network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailscaleDevice {
    pub id: String,
    pub hostname: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub os: String,
    #[serde(default, rename = "clientVersion")]
    pub client_version: String,
    #[serde(default, rename = "lastSeen")]
    pub last_seen: Option<DateTime<Utc>>,
    #[serde(default, rename = "blocksIncomingConnections")]
    pub blocks_incoming: bool,
    #[serde(default)]
    pub authorized: bool,
    #[serde(default)]
    pub user: String,
}

impl TailscaleDevice {
    pub fn primary_ip(&self) -> Option<&str> {
        self.addresses.iter()
            .find(|a| a.starts_with("100."))
            .map(|s| s.as_str())
    }

    pub fn is_likely_online(&self) -> bool {
        if self.blocks_incoming { return false; }
        match &self.last_seen {
            None => false,
            Some(ts) => {
                let age = Utc::now().signed_duration_since(*ts);
                age.num_minutes() < 5
            }
        }
    }

    pub fn display_name(&self) -> &str {
        if !self.name.is_empty() && self.name != self.hostname {
            &self.name
        } else {
            &self.hostname
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TailscaleDevicesResponse {
    pub devices: Vec<TailscaleDevice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteFile {
    pub name: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct TransferLogEntry {
    pub timestamp: DateTime<Utc>,
    pub message: String,
    pub level: TransferLogLevel,
}

impl TransferLogEntry {
    pub fn ok(msg: impl Into<String>) -> Self {
        Self { timestamp: Utc::now(), message: msg.into(), level: TransferLogLevel::Ok }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Self { timestamp: Utc::now(), message: msg.into(), level: TransferLogLevel::Error }
    }
    pub fn info(msg: impl Into<String>) -> Self {
        Self { timestamp: Utc::now(), message: msg.into(), level: TransferLogLevel::Info }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransferLogLevel { Ok, Error, Info }

#[derive(Debug, Clone)]
pub struct FileQueueItem {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub status: FileTransferStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FileTransferStatus { Pending, Sending, Done, Failed(String) }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardEntry {
    pub content: String,
    pub sender: String,
    pub received_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentPingResponse {
    pub ok: bool,
    pub hostname: String,
    pub device: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSearchResult {
    pub id: i64,
    pub device_id: String,
    pub device_name: String,
    pub path: PathBuf,
    pub name: String,
    pub ext: Option<String>,
    pub size: u64,
    pub modified: Option<i64>,
    pub rank: Option<f64>,
}

impl FileSearchResult {
    pub fn display_path(&self) -> String {
        format!(
            "{} › {}",
            self.device_name,
            self.path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeTelemetry {
    pub cpu_pct: f32,
    pub ram_used: u64,
    pub ram_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
    pub cpu_temp: Option<f32>,
    pub is_ai_capable: bool,
}

impl NodeTelemetry {
    pub fn ram_pct(&self) -> f32 {
        if self.ram_total == 0 { return 0.0; }
        self.ram_used as f32 / self.ram_total as f32 * 100.0
    }
    pub fn disk_pct(&self) -> f32 {
        if self.disk_total == 0 { return 0.0; }
        self.disk_used as f32 / self.disk_total as f32 * 100.0
    }
}

#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub total_files: u64,
    pub local_files: u64,
    pub last_scanned: Option<i64>,
    pub scanning: bool,
    pub scan_progress: u64,
    pub scan_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalEntry {
    pub file_id:     i64,
    pub device_id:   String,
    pub device_name: String,
    pub path:        PathBuf,
    pub name:        String,
    pub ext:         Option<String>,
    pub size:        u64,
    pub modified:    i64,
    pub event_kind:  TemporalEventKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TemporalEventKind {
    Created,
    Modified,
    Deleted,
}

impl TemporalEventKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Created  => "CREATED",
            Self::Modified => "MODIFIED",
            Self::Deleted  => "DELETED",
        }
    }
    pub fn glyph(&self) -> &'static str {
        match self {
            Self::Created  => "⊕",
            Self::Modified => "⊙",
            Self::Deleted  => "⊘",
        }
    }
}
