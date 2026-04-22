use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DetectionSource {
    #[default]
    FullScan,
    Watcher,
    Sync,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct DetectionSourceDistribution {
    pub full_scan: u64,
    pub watcher: u64,
    pub sync: u64,
}

impl DetectionSourceDistribution {
    pub fn increment(&mut self, source: DetectionSource) {
        match source {
            DetectionSource::FullScan => self.full_scan += 1,
            DetectionSource::Watcher => self.watcher += 1,
            DetectionSource::Sync => self.sync += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.full_scan + self.watcher + self.sync
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncHealthMetrics {
    pub observed_at: i64,
    pub last_sync_at: Option<i64>,
    pub sync_age_secs: Option<u64>,
    pub tombstone_count: u64,
    pub sync_failures: u64,
    pub detection_sources: DetectionSourceDistribution,
}

impl SyncHealthMetrics {
    pub fn mark_sync_success(
        &mut self,
        at_ts: i64,
        tombstone_count: u64,
        detection_sources: DetectionSourceDistribution,
    ) {
        self.observed_at = at_ts;
        self.last_sync_at = Some(at_ts);
        self.sync_age_secs = Some(0);
        self.tombstone_count = tombstone_count;
        self.detection_sources = detection_sources;
    }

    pub fn mark_sync_failure(&mut self, at_ts: i64) {
        self.observed_at = at_ts;
        self.sync_failures += 1;
        self.sync_age_secs = self
            .last_sync_at
            .map(|last| (at_ts - last).max(0) as u64);
    }
}

impl DetectionSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FullScan => "full_scan",
            Self::Watcher => "watcher",
            Self::Sync => "sync",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "watcher" => Self::Watcher,
            "sync" => Self::Sync,
            _ => Self::FullScan,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub size: u64,
    pub modified: Option<i64>,
    pub quick_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub kind: FileChangeKind,
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub new_path: Option<PathBuf>,
    pub fingerprint: Option<FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTombstone {
    pub device_id: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<i64>,
    pub hash: Option<String>,
    pub quick_hash: Option<String>,
    pub deleted_at: i64,
    #[serde(default)]
    pub detected_by: DetectionSource,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncDelta {
    #[serde(default)]
    pub files: Vec<FileSearchResult>,
    #[serde(default)]
    pub tombstones: Vec<FileTombstone>,
}

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
            .map(|s| s.split('/').next().unwrap_or(s))
    }

    pub fn is_likely_online(&self) -> bool {
        if self.blocks_incoming { return false; }
        match &self.last_seen {
            None => false,
            Some(ts) => {
                let age = Utc::now().signed_duration_since(*ts);
                age.num_minutes() < 30
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
    #[serde(default)]
    pub is_dir: bool,
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
    #[serde(default)]
    pub authorized: bool,
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
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub quick_hash: Option<String>,
    #[serde(default)]
    pub indexed_at: i64,
    pub rank: Option<f64>,
}

impl FileSearchResult {
    pub fn display_path(&self) -> String {
        format!(
            "{} / {}",
            self.device_name,
            self.path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        )
    }
}

/// A delta of file index changes sent in response to a sync request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncDelta {
    #[serde(default)]
    pub files: Vec<FileSearchResult>,
    #[serde(default)]
    pub tombstones: Vec<FileTombstone>,
}

/// Physical disk/partition info.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriveInfo {
    pub name: String,
    pub used: u64,
    pub total: u64,
    pub kind: Option<String>,
}

/// Physical RAM module info.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RamModule {
    pub slot: String,
    pub capacity: u64,
    pub speed_mhz: Option<u32>,
    pub configured_speed_mhz: Option<u32>,
    pub memory_type: Option<String>,
    pub form_factor: Option<String>,
    pub latency_cl: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
}

/// GPU device info.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GpuDevice {
    pub name: String,
    #[serde(default)]
    pub is_discrete: bool,
    #[serde(default)]
    pub is_integrated: bool,
    #[serde(default)]
    pub is_shared: bool,
    #[serde(default)]
    pub is_rtx: bool,
    #[serde(default)]
    pub ai_capable: bool,
    pub vendor: Option<String>,
    pub bus_type: Option<String>,
    pub vram_type: Option<String>,
    pub gpu_pct: Option<f32>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceCapabilities {
    pub ai_models: Vec<String>,
    pub has_camera: bool,
    pub has_microphone: bool,
    pub has_speakers: bool,
    pub drives: Vec<DriveInfo>,
    pub has_rdp: bool,
    pub has_file_access: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeTelemetry {
    pub device_type: String,
    pub cpu_pct: f32,
    #[serde(default)]
    pub cpu_model: Option<String>,
    #[serde(default)]
    pub cpu_physical_cores: Option<u32>,
    #[serde(default)]
    pub cpu_logical_processors: Option<u32>,
    pub cpu_cores_pct: Option<Vec<f32>>,
    pub cpu_freq_ghz: Option<f32>,
    pub ram_used: u64,
    pub ram_total: u64,
    pub ram_slots_used: Option<u32>,
    pub ram_slots_total: Option<u32>,
    pub ram_speed_mhz: Option<u32>,
    pub ram_form_factor: Option<String>,
    #[serde(default)]
    pub ram_modules: Vec<RamModule>,
    pub disk_used: u64,
    pub disk_total: u64,
    pub cpu_temp: Option<f32>,
    pub is_ai_capable: bool,
    #[serde(default)]
    pub gpu_devices: Vec<GpuDevice>,
    pub gpu_name: Option<String>,
    pub gpu_pct: Option<f32>,
    pub gpu_mem_used: Option<u64>,
    pub gpu_mem_total: Option<u64>,
    pub local_ips: Vec<String>,
    pub running_processes: Option<u32>,
    #[serde(default)]
    pub top_processes: Vec<String>,
    pub ai_status: Option<String>,
    pub ai_tokens_per_sec: Option<f32>,
    pub ai_thoughts: Option<String>,
    pub capabilities: DeviceCapabilities,
    #[serde(default)]
    pub net_rx_bps: Option<u64>,
    #[serde(default)]
    pub net_tx_bps: Option<u64>,
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
    pub fn gpu_mem_pct(&self) -> f32 {
        match (self.gpu_mem_used, self.gpu_mem_total) {
            (Some(u), Some(t)) if t > 0 => u as f32 / t as f32 * 100.0,
            _ => 0.0,
        }
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
    pub scan_start_ts: Option<i64>,
    pub scan_eta_secs: Option<u64>,
    pub last_progress_ts: Option<i64>,
    pub last_progress_scanned: u64,
    pub smoothed_files_per_sec: Option<f64>,
    pub type_counts: std::collections::HashMap<String, u64>,
}

impl IndexStats {
    pub fn reset_scan(&mut self) {
        self.scanning = true;
        self.scan_progress = 0;
        self.scan_total = 0;
        self.scan_start_ts = Some(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64);
        self.scan_eta_secs = None;
        self.last_progress_ts = None;
        self.last_progress_scanned = 0;
        self.smoothed_files_per_sec = None;
        self.type_counts.clear();
    }
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
    pub hash:        Option<String>,
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
}

/// Sync health observability metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncHealthMetrics {
    pub observed_at:    i64,
    pub last_sync_at:   Option<i64>,
    pub sync_age_secs:  Option<u64>,
    pub tombstone_count: u64,
    pub sync_failures:  u64,
}

/// Kind of preview content for a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PreviewKind {
    Text,
    Image,
    Hex,
    Unsupported,
}

/// A rule for auto-tagging/categorising files.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserRule {
    pub id:        i64,
    pub name:      String,
    pub pattern:   String,
    pub project:   Option<String>,
    pub tag:       Option<String>,
    pub is_active: bool,
}

/// A tombstone representing a deleted file in the sync delta.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileTombstone {
    pub device_id: String,
    pub path: std::path::PathBuf,
    pub deleted_at: i64,
}

