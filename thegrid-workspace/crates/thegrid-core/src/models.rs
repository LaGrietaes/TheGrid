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
    /// Returns the best routable IP: prefers the Tailscale IPv4 (`100.x`),
    /// falls back to the IPv6 Tailscale range (`fd7a:`), then any address.
    pub fn primary_ip(&self) -> Option<&str> {
        fn strip(s: &str) -> &str { s.split('/').next().unwrap_or(s) }
        // 1. Tailscale IPv4 (always preferred — works with ping/http)
        if let Some(a) = self.addresses.iter().find(|a| a.starts_with("100.")) {
            return Some(strip(a));
        }
        // 2. Tailscale IPv6 (fd7a:) — still routable on the tailnet
        if let Some(a) = self.addresses.iter().find(|a| a.starts_with("fd7a:") || a.starts_with("fd")) {
            return Some(strip(a));
        }
        // 3. Any address — surface it so the device is visible even if unusual
        self.addresses.first().map(|a| strip(a))
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
    pub hash: Option<String>,
    pub quick_hash: Option<String>,
    pub indexed_at: i64,
    #[serde(default)]
    pub detected_by: DetectionSource,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateScanFilter {
    #[serde(default)]
    pub min_size_bytes: u64,
    #[serde(default)]
    pub include_extensions: Vec<String>,
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default = "DuplicateScanFilter::default_true")]
    pub exclude_system_paths: bool,
    #[serde(default = "DuplicateScanFilter::default_max_groups")]
    pub max_groups: usize,
}

impl DuplicateScanFilter {
    fn default_true() -> bool {
        true
    }

    fn default_max_groups() -> usize {
        200
    }
}

impl Default for DuplicateScanFilter {
    fn default() -> Self {
        Self {
            min_size_bytes: 0,
            include_extensions: Vec::new(),
            path_prefix: None,
            device_id: None,
            exclude_system_paths: true,
            max_groups: Self::default_max_groups(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveBufferEntry {
    pub source_path: PathBuf,
    pub staged_path: PathBuf,
    pub device_id: String,
    pub category: String,
    pub hash: String,
    pub size: u64,
    pub duplicate_group_size: usize,
    pub indexed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveBufferManifest {
    pub generated_at: i64,
    pub session_id: String,
    pub quota_tb: u32,
    pub source_groups: usize,
    pub source_files: usize,
    pub staged_files: usize,
    pub staged_total_bytes: u64,
    pub root_folder: PathBuf,
    pub entries: Vec<DriveBufferEntry>,
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
    /// Network throughput in bytes/sec (last sample interval)
    #[serde(default)]
    pub net_rx_bps: Option<u64>,
    #[serde(default)]
    pub net_tx_bps: Option<u64>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriveInfo {
    pub name: String,
    pub used: u64,
    pub total: u64,
    pub kind: Option<String>,
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
    #[serde(default)]
    pub compute: ComputeCapabilities,
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
        self.scan_start_ts = Some(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64);
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
    pub fn glyph(&self) -> &'static str {
        match self {
            Self::Created  => "⊕",
            Self::Modified => "⊙",
            Self::Deleted  => "⊘",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRule {
    pub id: i64,
    pub name: String,
    pub pattern: String,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleMatch {
    pub rule_id: i64,
    pub file_id: i64,
    pub tag: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum PreviewKind {
    #[default]
    None,
    Text,
    Image,
    Psd,
    Pdf,
    UnSupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SmartFilterType {
    Extension(String),
    MinSize(u64),
    MaxSize(u64),
    ModifiedAfter(chrono::DateTime<chrono::Utc>),
    ModifiedBefore(chrono::DateTime<chrono::Utc>),
    Project(String),
    Category(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SmartRule {
    pub id: String,
    pub name: String,
    pub filters: Vec<SmartFilterType>,
}

// ── Compute Sharing ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ComputeTaskType {
    TextEmbedding,
    ImageEmbedding,
    FullHash,
    LocalLlm,
}

impl std::fmt::Display for ComputeTaskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextEmbedding  => write!(f, "TEXT EMBED"),
            Self::ImageEmbedding => write!(f, "IMG EMBED"),
            Self::FullHash       => write!(f, "HASH"),
            Self::LocalLlm       => write!(f, "LLM"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComputeCapabilities {
    pub gpu_available: bool,
    pub gpu_models: Vec<String>,
    pub gpu_vram_mb: u64,
    pub cpu_cores: u32,
    pub ram_available_mb: u64,
    pub max_parallel_tasks: u8,
    pub supported_task_types: Vec<ComputeTaskType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComputePayload {
    TextEmbed { text: String },
    ImageEmbed { file_url: String },
    FullHash   { file_url: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeTaskRequest {
    pub task_id: String,
    pub task_type: ComputeTaskType,
    pub requester_device_id: String,
    pub requester_callback_url: String,
    pub payload: ComputePayload,
    pub priority: u8,
    pub deadline_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeTaskReceipt {
    pub task_id: String,
    pub accepted: bool,
    pub reason_if_rejected: Option<String>,
    pub eta_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputeTaskState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeTaskProgress {
    pub task_id: String,
    pub state: ComputeTaskState,
    pub pct: u8,
    pub result_uri: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeStatus {
    pub available: bool,
    pub active_tasks: u32,
    pub queued_tasks: u32,
    pub max_parallel_tasks: u8,
    pub busy_until_estimate_secs: Option<u32>,
}

/// In-memory record of an active borrow relationship between two devices.
#[derive(Debug, Clone)]
pub struct ComputeSession {
    pub borrower_device_id: String,
    pub provider_device_id: String,
    pub task_type: ComputeTaskType,
    pub task_id: String,
    pub started_at: std::time::Instant,
}

// ── DeviceDisplayState ────────────────────────────────────────────────────────

/// Derived state for GUI device card rendering.
/// Precedence (highest → lowest): Error > ComputeBorrowing > ComputeProviding >
/// Indexing > Syncing > Busy > Online > Offline.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DeviceDisplayState {
    #[default]
    Offline,
    Online,
    Syncing,
    Indexing,
    ComputeBorrowing,
    ComputeProviding,
    Busy,
    Error(String),
}

impl DeviceDisplayState {
    pub fn label(&self) -> &str {
        match self {
            Self::Offline          => "offline",
            Self::Online           => "online",
            Self::Syncing          => "syncing",
            Self::Indexing         => "indexing",
            Self::ComputeBorrowing => "borrowing compute",
            Self::ComputeProviding => "providing compute",
            Self::Busy             => "busy",
            Self::Error(_)         => "error",
        }
    }

    pub fn precedence(&self) -> u8 {
        match self {
            Self::Error(_)         => 8,
            Self::ComputeBorrowing => 7,
            Self::ComputeProviding => 6,
            Self::Indexing         => 5,
            Self::Syncing          => 4,
            Self::Busy             => 3,
            Self::Online           => 2,
            Self::Offline          => 1,
        }
    }
}

// ── Indexing Policy ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IndexingTier {
    /// Never index: .git/objects/, node_modules/, target/, etc.
    Tier0Exclude,
    /// Metadata only: no full hash, no embedding.
    Tier1Deprioritized,
    /// Git working copy with a GitHub remote — recoverable, skipped from dedup.
    GitHubBacked,
    /// Index fully.
    #[default]
    FullIndex,
}

impl IndexingTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tier0Exclude       => "Tier0Exclude",
            Self::Tier1Deprioritized => "Tier1Deprioritized",
            Self::GitHubBacked       => "GitHubBacked",
            Self::FullIndex          => "FullIndex",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideAction {
    ForceInclude,
    ForceExclude,
    MetadataOnly,
    DeprioritizeTier1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingOverride {
    pub path_pattern: String,
    pub action: OverrideAction,
}

// ── Duplicate groups ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Local,
    GoogleDrive,
    Nas,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSummary {
    pub device_id: String,
    pub device_name: String,
    pub file_count: u32,
    pub source_type: SourceType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub hash: String,
    pub size: u64,
    pub file_count: u32,
    pub source_count: u32,
    pub sources: Vec<SourceSummary>,
    pub files: Vec<FileSearchResult>,
    /// device_id of the preferred anchor (keep candidate).
    pub suggested_anchor: Option<String>,
}

// ── Deletion audit ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeletionRecord {
    pub id: i64,
    pub session_id: String,
    pub file_path: String,
    pub device_id: String,
    pub file_hash: Option<String>,
    pub file_size: Option<u64>,
    pub action: String,
    pub reason: Option<String>,
    pub executed_at: i64,
}

// ── Extend DeviceCapabilities ─────────────────────────────────────────────────

// DeviceCapabilities is defined above; we re-open the struct by adding the compute
// field via the existing definition. Since Rust structs can't be reopened, we add the
// field directly in the original definition instead — done via a separate alias here.
// The actual extended struct replaces the original in models.rs during Phase 3.

// ── Google Drive ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveFileMetadata {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
    pub md5_checksum: Option<String>,
    pub mime_type: String,
    pub parents: Vec<String>,
    pub is_shared_drive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DriveAbout {
    pub email: String,
    pub storage_used: u64,
    pub storage_limit: Option<u64>,
}

