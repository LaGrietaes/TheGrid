use crate::models::*;
use crate::config::Config;
use std::path::PathBuf;
use std::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    // Tailscale
    DevicesLoaded(Vec<TailscaleDevice>),
    DevicesFailed(String),

    // Agent (remote THE GRID instance)
    AgentPingOk(AgentPingResponse),
    AgentPingFailed(String),
    RemoteFilesLoaded(Vec<RemoteFile>),
    RemoteFilesFailed(String),

    RemoteBrowseLoaded {
        device_id: String,
        path:      PathBuf,
        files:     Vec<RemoteFile>,
    },
    RemoteBrowseFailed {
        device_id: String,
        error:     String,
    },
    RemoteConfigUpdated { device_id: String },
    RemoteConfigFailed  { device_id: String, error: String },

    // Remote Terminal
    RemoteTerminalCreated { device_id: String, session_id: String },
    RemoteTerminalFailed  { device_id: String, error: String },
    RemoteTerminalOutput  { device_id: String, data: Vec<u8> },

    // File Transfer
    FileSent        { queue_idx: usize, name: String },
    FileSendFailed  { queue_idx: usize, error: String },
    FileDownloaded  { name: String, path: PathBuf },
    FileDownloadFailed { name: String, error: String },

    // Clipboard
    ClipboardSent,
    ClipboardSendFailed(String),
    ClipboardReceived(ClipboardEntry),
    FileReceived { name: String, size: u64 },

    // Config
    SetupComplete(Config),
    SetupFailed(String),

    // Filesystem Watcher
    FileSystemChanged {
        paths:   Vec<PathBuf>,
        summary: String,
    },
    FileWatcherError(String),

    // Phase 3: File Index
    IndexProgress {
        scanned: u64,
        total:   u64,
        current: String,
        ext:     Option<String>,
        estimated_total: bool,
    },

    /// Incoming request from a remote node for an index sync.
    SyncRequest {
        after:            i64,
        requester_device: Option<String>,
        response_tx:      mpsc::Sender<SyncDelta>,
    },

    SyncComplete { device_id: String, files_added: usize },
    SyncFailed   { device_id: String, error: String },

    SyncHealthUpdated {
        device_id: String,
        metrics:   SyncHealthMetrics,
    },

    SemanticReady,
    SemanticFailed(String),

    EmbeddingProgress { indexed: usize, total: usize },
    HashingProgress   { hashed: usize,  total: usize },

    IndexComplete { device_id: String, files_added: u64, duration_ms: u64 },
    IndexUpdated  { paths_updated: usize },

    SearchResults(Vec<FileSearchResult>),
    DuplicatesFound(Vec<(String, u64, Vec<FileSearchResult>)>),

    TelemetryUpdate {
        device_id: String,
        ip:        Option<String>,
        telemetry: NodeTelemetry,
    },

    WolSent   { device_name: String, target_mac: String },
    WolFailed { reason: String },

    TemporalLoaded(Vec<TemporalEntry>),

    /// Incoming AI embed request from a remote node via AgentServer.
    RemoteAiEmbedRequest {
        text:        String,
        response_tx: mpsc::Sender<Vec<f32>>,
    },

    /// Incoming AI semantic search request from a remote node via AgentServer.
    RemoteAiSearchRequest {
        query:       String,
        k:           usize,
        response_tx: mpsc::Sender<Vec<(i64, f32)>>,
    },

    // Status
    Status(String),

    // UI
    RequestRefresh,
    OpenSettings,

    // ADB Mirroring Preparation
    EnableAdb { ip: String, api_key: String },

    // RDP Support
    EnableRdp  { ip: String, device_id: String },
    RdpEnabled { device_id: String },
    RdpFailed  { device_id: String, error: String },

    // AI Lifecycle
    RefreshAiServices,

    // Preview
    RequestFilePreview(FileSearchResult),
    FilePreviewLoaded  { file_id: i64, content: String, kind: PreviewKind },

    // File Manager Operations
    DeleteFiles { device_id: String, paths: Vec<String> },
    RenameFile  { device_id: String, old_path: String, new_name: String },
    MoveFiles   { device_id: String, paths: Vec<String>, dest_dir: String },

    // Phase 4: Persistence & Idle
    UserIdle(bool),
    RequestIdleWork,
}
