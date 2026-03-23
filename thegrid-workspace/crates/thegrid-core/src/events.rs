use crate::models::*;
use crate::config::Config;
use std::path::PathBuf;
use std::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    // ── Tailscale ──────────────────────────────────────────────────────────
    /// Devices fetched successfully from Tailscale API
    DevicesLoaded(Vec<TailscaleDevice>),

    /// Tailscale API call failed
    DevicesFailed(String),

    // ── Agent (remote THE GRID instance) ──────────────────────────────────
    /// Ping to a remote agent succeeded
    AgentPingOk { ip: String, response: AgentPingResponse, manual: bool },

    /// Ping to a remote agent failed
    AgentPingFailed { ip: String, error: String, manual: bool },

    /// Remote file list fetched
    RemoteFilesLoaded(Vec<RemoteFile>),

    /// Remote file list fetch failed
    RemoteFilesFailed(String),

    /// A remote file preview was loaded
    AgentFilePreviewLoaded(Vec<u8>),

    /// A directory list was fetched from a remote node
    RemoteBrowseLoaded {
        device_id: String,
        path:      PathBuf,
        files:     Vec<RemoteFile>,
    },

    /// A remote directory browse failed
    RemoteBrowseFailed {
        device_id: String,
        error:     String,
    },

    /// Remote configuration was updated successfully
    RemoteConfigUpdated {
        device_id: String,
    },

    /// Remote configuration update failed
    RemoteConfigFailed {
        device_id: String,
        error:     String,
    },

    // ── Remote Terminal ────────────────────────────────────────────────────
    /// Terminal session created
    RemoteTerminalCreated {
        device_id:  String,
        session_id: String,
    },

    /// Terminal session creation failed
    RemoteTerminalFailed {
        device_id: String,
        error:     String,
    },

    /// Incoming terminal output
    RemoteTerminalOutput {
        device_id: String,
        data:      Vec<u8>,
    },

    // ── File Transfer ──────────────────────────────────────────────────────
    /// A file was sent successfully (queue index, file name)
    FileSent { queue_idx: usize, name: String },

    /// A file send failed (queue index, error)
    FileSendFailed { queue_idx: usize, error: String },

    /// A file download completed
    FileDownloaded { name: String, path: PathBuf },

    /// A file download failed
    FileDownloadFailed { name: String, error: String },

    // ── Clipboard ──────────────────────────────────────────────────────────
    /// Clipboard successfully pushed to remote device
    ClipboardSent,

    /// Clipboard send failed
    ClipboardSendFailed(String),

    /// Incoming clipboard from a remote device
    ClipboardReceived(ClipboardEntry),

    /// A file was received by the local agent
    FileReceived { name: String, size: u64 },

    // ── Config ─────────────────────────────────────────────────────────────
    /// Config validated and saved
    SetupComplete(Config),

    /// Config validation failed
    SetupFailed(String),

    // ── Filesystem Watcher ─────────────────────────────────────────────────
    /// One or more files changed in a watched directory.
    FileSystemChanged {
        paths:  Vec<PathBuf>,
        summary: String,
    },

    /// The filesystem watcher encountered a fatal error
    FileWatcherError(String),

    // ── Phase 3: File Index ────────────────────────────────────────────────
    IndexProgress {
        scanned: u64,
        total:   u64,
        current: String,
    },

    /// Incoming request from a remote node for an index sync.
    SyncRequest {
        after: i64,
        response_tx: mpsc::Sender<Vec<FileSearchResult>>,
    },

    /// Index synchronization completed.
    SyncComplete {
        device_id:   String,
        files_added: usize,
    },

    /// Index synchronization failed.
    SyncFailed {
        device_id: String,
        error:     String,
    },

    /// Semantic search engine is initialized.
    SemanticReady,

    /// Semantic initialization failed.
    SemanticFailed(String),

    /// Progress of the local background embedding generator.
    EmbeddingProgress {
        indexed: usize,
        total:   usize,
    },

    /// Progress of the local background hashing generator.
    HashingProgress {
        hashed: usize,
        total:  usize,
    },

    /// A full directory scan completed.
    IndexComplete {
        device_id:   String,
        files_added: u64,
        duration_ms: u64,
    },

    /// An incremental index update.
    IndexUpdated {
        paths_updated: usize,
    },

    /// Search results are ready.
    SearchResults(Vec<FileSearchResult>),

    /// Telemetry snapshot from a remote THE GRID agent.
    TelemetryUpdate {
        device_id:  String,
        ip:         Option<String>,
        telemetry:  NodeTelemetry,
    },

    /// Wake-on-LAN magic packet was sent.
    WolSent { device_name: String, target_mac: String },

    /// Wake-on-LAN failed.
    WolFailed { reason: String },

    /// Temporal view data loaded.
    TemporalLoaded(Vec<TemporalEntry>),

    /// Incoming request from a remote node via AgentServer to generate an embedding.
    RemoteAiEmbedRequest {
        text: String,
        response_tx: mpsc::Sender<Vec<f32>>,
    },

    /// Incoming request from a remote node via AgentServer to perform a semantic search.
    RemoteAiSearchRequest {
        query: String,
        k:     usize,
        response_tx: mpsc::Sender<Vec<(i64, f32)>>,
    },

    // ── Status ─────────────────────────────────────────────────────────────
    Status(String),

    // ── UI ─────────────────────────────────────────────────────────────────
    RequestRefresh,
    OpenSettings,

    // ADB Mirroring Preparation
    EnableAdb { ip: String, api_key: String },

    // RDP Support
    EnableRdp { ip: String, device_id: String },
    RdpEnabled { device_id: String },
    RdpFailed { device_id: String, error: String },
    
    // AI Lifecycle
    RefreshAiServices,

    // ── Preview ────────────────────────────────────────────────────────────
    /// Request a preview for a file
    RequestFilePreview(FileSearchResult),
    
    /// File preview content loaded
    FilePreviewLoaded {
        file_id: i64,
        content: String,
        kind:    PreviewKind,
    },
    
    // ── File Manager Operations ────────────────────────────────────────────
    DeleteFiles { device_id: String, paths: Vec<String> },
    RenameFile  { device_id: String, old_path: String, new_name: String },
    MoveFiles   { device_id: String, paths: Vec<String>, dest_dir: String },
}
