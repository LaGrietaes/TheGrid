use crate::models::*;
use crate::config::Config;
use std::path::PathBuf;
use std::sync::mpsc;

// ─────────────────────────────────────────────────────────────────────────────
// AppEvent — THE GRID's central message bus.
//
// Every variant here represents a state transition that the GUI may need to
// render. Variants marked GUI_HOOK describe exactly which screen/panel/widget
// should respond to that event.
//
// Convention:
//   GUI_HOOK: <Screen> → <Panel/Widget> — <what to update>
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Debug)]
pub enum AppEvent {
    // ── Tailscale ──────────────────────────────────────────────────────────
    // GUI_HOOK: Dashboard → NodeGrid — refresh device cards, status dots, IP badges
    /// Devices fetched successfully from Tailscale API
    DevicesLoaded(Vec<TailscaleDevice>),

    // GUI_HOOK: Dashboard → StatusBar / NodeGrid — show error banner + retry button
    /// Tailscale API call failed
    DevicesFailed(String),

    // ── Agent (remote THE GRID instance) ──────────────────────────────────
    // GUI_HOOK: Dashboard → NodeCard — set ◉ LIVE indicator; enable RDP/Terminal/Files buttons
    /// Ping to a remote agent succeeded
    AgentPingOk { ip: String, response: AgentPingResponse, manual: bool },

    // GUI_HOOK: Dashboard → NodeCard — set ◌ OFFLINE indicator; disable action buttons
    /// Ping to a remote agent failed
    AgentPingFailed { ip: String, error: String, manual: bool },

    // GUI_HOOK: FileManager → RemoteFileList panel — populate file rows
    /// Remote file list fetched
    RemoteFilesLoaded(Vec<RemoteFile>),

    // GUI_HOOK: FileManager → RemoteFileList — show inline error row
    /// Remote file list fetch failed
    RemoteFilesFailed(String),

    // GUI_HOOK: FileManager → PreviewPane — render image/text preview
    /// A remote file preview was loaded
    AgentFilePreviewLoaded(Vec<u8>),

    // GUI_HOOK: FileManager → DirectoryBrowser — populate directory tree/list
    /// A directory list was fetched from a remote node
    RemoteBrowseLoaded {
        device_id: String,
        path:      PathBuf,
        files:     Vec<RemoteFile>,
    },

    // GUI_HOOK: FileManager → DirectoryBrowser — show error toast + retry button
    /// A remote directory browse failed
    RemoteBrowseFailed {
        device_id: String,
        error:     String,
    },

    // GUI_HOOK: NodeCard → SettingsModal — show "Config saved" confirmation
    /// Remote configuration was updated successfully
    RemoteConfigUpdated {
        device_id: String,
    },

    // GUI_HOOK: NodeCard → SettingsModal — show error inline
    /// Remote configuration update failed
    RemoteConfigFailed {
        device_id: String,
        error:     String,
    },

    // GUI_HOOK: NodeCard → UpdateButton — show "Update started" spinner
    /// Remote node update (git pull + rebuild) was triggered successfully
    RemoteUpdateStarted {
        device_id: String,
    },

    // GUI_HOOK: NodeCard → UpdateButton — show error toast
    /// Remote node update trigger failed (agent unreachable or rejected)
    RemoteUpdateFailed {
        device_id: String,
        error:     String,
    },

    // ── Remote Terminal ────────────────────────────────────────────────────
    // GUI_HOOK: Terminal screen — store session_id; begin input/output polling loop
    /// Terminal session created
    RemoteTerminalCreated {
        device_id:  String,
        session_id: String,
    },

    // GUI_HOOK: Terminal screen — show error; offer "Retry" button
    /// Terminal session creation failed
    RemoteTerminalFailed {
        device_id: String,
        error:     String,
    },

    // GUI_HOOK: Terminal screen → TerminalView — append bytes to PTY buffer
    /// Incoming terminal output
    RemoteTerminalOutput {
        device_id: String,
        data:      Vec<u8>,
    },

    // ── File Transfer ──────────────────────────────────────────────────────
    // GUI_HOOK: FileManager → TransferQueue — mark row ✓ SENT; update progress bar
    /// A file was sent successfully (queue index, file name)
    FileSent { queue_idx: usize, name: String },

    // GUI_HOOK: FileManager → TransferQueue — mark row ✗ FAILED; show error inline
    /// A file send failed (queue index, error)
    FileSendFailed { queue_idx: usize, error: String },

    // GUI_HOOK: FileManager → IncomingFiles panel — add row; offer "Open in Explorer"
    /// A file download completed
    FileDownloaded { name: String, path: PathBuf },

    // GUI_HOOK: FileManager → IncomingFiles — show error row
    /// A file download failed
    FileDownloadFailed { name: String, error: String },

    // ── Clipboard ──────────────────────────────────────────────────────────
    // GUI_HOOK: Dashboard → ClipboardWidget — flash "Sent ✓" for 2s
    /// Clipboard successfully pushed to remote device
    ClipboardSent,

    // GUI_HOOK: Dashboard → ClipboardWidget — show red error banner
    /// Clipboard send failed
    ClipboardSendFailed(String),

    // GUI_HOOK: Dashboard → ClipboardInbox — prepend entry; badge count +1
    /// Incoming clipboard from a remote device
    ClipboardReceived(ClipboardEntry),

    // GUI_HOOK: Dashboard → IncomingFilesWidget — prepend row with filename + size
    /// A file was received by the local agent
    FileReceived { name: String, size: u64 },

    // ── Config ─────────────────────────────────────────────────────────────
    // GUI_HOOK: SetupScreen → transition to Dashboard; store Config
    /// Config validated and saved
    SetupComplete(Config),

    // GUI_HOOK: SetupScreen → inline validation error under offending field
    /// Config validation failed
    SetupFailed(String),

    // ── Filesystem Watcher ─────────────────────────────────────────────────
    // GUI_HOOK: StatusBar → "N changes detected" badge; trigger incremental re-index
    /// One or more files changed in a watched directory.
    FileSystemChanged {
        changes: Vec<FileChange>,
        summary: String,
    },

    // GUI_HOOK: StatusBar → watcher error icon; Settings → WatchPaths section error
    /// The filesystem watcher encountered a fatal error
    FileWatcherError(String),

    // ── File Index ────────────────────────────────────────────────────────
    // GUI_HOOK: StatusBar → IndexProgress bar (scanned / total, current filename)
    IndexProgress {
        scanned: u64,
        total:   u64,
        current: String,
        ext:     Option<String>,
        estimated_total: bool,
    },

    // GUI_HOOK: handled internally (runtime responds with SyncDelta over response_tx) — no GUI widget
    /// Incoming request from a remote node for an index sync.
    SyncRequest {
        after: i64,
        requester_device: Option<String>,
        response_tx: mpsc::Sender<SyncDelta>,
    },

    // GUI_HOOK: Dashboard → MeshStatus section — update peer row (files_added, last sync time)
    /// Index synchronization completed.
    SyncComplete {
        device_id:   String,
        files_added: usize,
    },

    // GUI_HOOK: Dashboard → MeshStatus — mark peer row ⚠ SYNC ERROR
    /// Index synchronization failed.
    SyncFailed {
        device_id: String,
        error:     String,
    },

    // GUI_HOOK: Dashboard → MeshStatus — per-peer age gauge, tombstone count, detection breakdown
    /// Sync observability snapshot for operator-facing health views.
    SyncHealthUpdated {
        device_id: String,
        metrics:   SyncHealthMetrics,
    },

    // GUI_HOOK: Search panel → AI mode indicator turns ◉ READY (blue border)
    /// Semantic search engine is initialized.
    SemanticReady,

    // GUI_HOOK: Search panel → show "AI unavailable — keyword mode" warning
    /// Semantic initialization failed.
    SemanticFailed(String),

    // GUI_HOOK: StatusBar → "Embedding N/M" progress chip (shown in background tasks drawer)
    /// Progress of the local background embedding generator.
    EmbeddingProgress {
        indexed: usize,
        total:   usize,
    },

    // GUI_HOOK: StatusBar → "Hashing N/M" progress chip
    /// Progress of the local background hashing generator.
    HashingProgress {
        hashed: usize,
        total:  usize,
    },

    // GUI_HOOK: StatusBar → dismiss IndexProgress bar; show "Index complete: N files" toast
    /// A full directory scan completed.
    IndexComplete {
        device_id:   String,
        files_added: u64,
        duration_ms: u64,
    },

    // GUI_HOOK: StatusBar → "Index updated" flash badge (subtle, not a modal)
    /// An incremental index update.
    IndexUpdated {
        paths_updated: usize,
    },

    // GUI_HOOK: Search panel → SearchResults list — populate rows with file cards
    /// Search results are ready.
    SearchResults(Vec<FileSearchResult>),

    // GUI_HOOK: DedupReview screen — populate duplicate group cards with Keep/Delete actions
    /// Duplicate file groups found: each entry is (hash, size_bytes, files).
    DuplicatesFound(Vec<(String, u64, Vec<FileSearchResult>)>),

    // GUI_HOOK: Dashboard → NodeCard telemetry band — CPU/RAM/disk gauges; drive info
    /// Telemetry snapshot from a remote THE GRID agent.
    TelemetryUpdate {
        device_id:  String,
        ip:         Option<String>,
        telemetry:  NodeTelemetry,
    },

    // GUI_HOOK: NodeCard → WoL button — show "Packet sent" confirmation toast
    /// Wake-on-LAN magic packet was sent.
    WolSent { device_name: String, target_mac: String },

    // GUI_HOOK: NodeCard → WoL button — show error inline
    /// Wake-on-LAN failed.
    WolFailed { reason: String },

    // GUI_HOOK: Timeline screen → "The Flow" — day-separated file list with relative timestamps
    /// Temporal view data loaded.
    TemporalLoaded(Vec<TemporalEntry>),

    // GUI_HOOK: internal — runtime handles this synchronously; no direct GUI widget needed
    /// Incoming request from a remote node via AgentServer to generate an embedding.
    RemoteAiEmbedRequest {
        text: String,
        response_tx: mpsc::Sender<Vec<f32>>,
    },

    // GUI_HOOK: internal — runtime handles this synchronously; no direct GUI widget needed
    /// Incoming request from a remote node via AgentServer to perform a semantic search.
    RemoteAiSearchRequest {
        query: String,
        k:     usize,
        response_tx: mpsc::Sender<Vec<(i64, f32)>>,
    },

    // ── Status ─────────────────────────────────────────────────────────────
    // GUI_HOOK: StatusBar → status line text; Boot screen → log feed; Node TUI → log section
    Status(String),

    // ── UI ─────────────────────────────────────────────────────────────────
    // GUI_HOOK: App → request egui repaint
    RequestRefresh,
    // GUI_HOOK: App → open Settings modal overlay
    OpenSettings,

    // GUI_HOOK: NodeCard → ADB button — show "Enabling…" spinner; transition to Terminal
    EnableAdb { ip: String, api_key: String },

    // GUI_HOOK: NodeCard → RDP button — show "Enabling…" spinner
    EnableRdp { ip: String, device_id: String },
    // GUI_HOOK: NodeCard → RDP button — switch to "Launch RDP" state
    RdpEnabled { device_id: String },
    // GUI_HOOK: NodeCard → RDP button — show error inline
    RdpFailed { device_id: String, error: String },

    // GUI_HOOK: internal — triggers runtime to re-init AI providers; no direct widget
    RefreshAiServices,

    // ── Preview ────────────────────────────────────────────────────────────
    // GUI_HOOK: internal — triggers background load; result comes back as FilePreviewLoaded
    /// Request a preview for a file
    RequestFilePreview(FileSearchResult),

    // GUI_HOOK: PreviewPane / MediaIngest → PreviewPanel — render text or image
    /// File preview content loaded
    FilePreviewLoaded {
        file_id: i64,
        content: String,
        kind:    PreviewKind,
    },

    // ── File Manager Operations ────────────────────────────────────────────
    // GUI_HOOK: FileManager → after success, refresh directory listing; show toast
    DeleteFiles { device_id: String, paths: Vec<String> },
    // GUI_HOOK: FileManager → rename inline field; refresh row on success
    RenameFile  { device_id: String, old_path: String, new_name: String },
    // GUI_HOOK: FileManager → move dialog; refresh source + dest directories on success
    MoveFiles   { device_id: String, paths: Vec<String>, dest_dir: String },

    // GUI_HOOK: Sentinel drives this — runtime updates stance; GUI reads via SecurityStanceChanged
    UserIdle(bool), // true if idle > threshold
    // GUI_HOOK: internal — triggers background dedup/hash/embedding passes; no direct widget
    RequestIdleWork,

    // ── Compute sharing ────────────────────────────────────────────────────
    // GUI_HOOK: Dashboard → ComputeWidget — show "Task delegated to <peer>" chip
    /// This device successfully delegated a task to a remote peer.
    ComputeBorrowOk {
        task_id: String,
        provider_device_id: String,
        task_type: crate::models::ComputeTaskType,
    },

    // GUI_HOOK: Dashboard → ComputeWidget — show "Executing locally" fallback indicator
    /// Compute borrow attempt failed (peer rejected or timed out).
    ComputeBorrowFailed {
        task_id: String,
        reason: String,
    },

    // GUI_HOOK: Dashboard → ComputeWidget — update task progress bar (pct, state)
    /// Progress update for an in-flight compute task (local or borrowed).
    ComputeTaskUpdate(crate::models::ComputeTaskProgress),

    // ── Google Drive ───────────────────────────────────────────────────────
    // GUI_HOOK: Settings → DriveSection — show "Reconnect Google Drive" CTA button
    /// Drive OAuth token expired or missing — user must re-authenticate.
    DriveAuthExpired,

    // GUI_HOOK: StatusBar → "Drive indexing N/M" progress chip
    /// Drive metadata indexing progress.
    DriveIndexProgress { indexed: u64, total: Option<u64> },

    // GUI_HOOK: StatusBar → dismiss Drive progress chip; show "Drive index ready" toast
    /// Drive metadata indexing complete.
    DriveIndexComplete { indexed: u64 },

    // GUI_HOOK: StatusBar → "Drive error" warning badge; Settings → Drive section error
    /// Drive indexing encountered a non-fatal error.
    DriveIndexError(String),

    // ── Duplicate groups (rich cross-source format) ────────────────────────
    // GUI_HOOK: DedupReview screen — populate group cards with source badges and anchor suggestion
    /// Rich duplicate groups ready for review UI (emitted after a live scan).
    DuplicatesGrouped(Vec<crate::models::DuplicateGroup>),

    // GUI_HOOK: DedupReview screen — restore group cards with prior Keep/Delete decisions pre-filled
    /// Persisted duplicate groups restored from the DB on startup or explicit reload.
    DuplicateGroupsRestored(Vec<crate::models::DuplicateGroup>, std::collections::HashMap<i64, String>),

    // ── Local AI / Ollama ──────────────────────────────────────────────────
    // GUI_HOOK: Settings → OllamaSection — populate model dropdown; show available / pull buttons
    /// Background probe returned list of locally available Ollama model names.
    OllamaModelsDetected(Vec<String>),

    // GUI_HOOK: Settings → OllamaSection → LoadModel button — show pull progress
    /// User clicked "Load Model" — pull a model into Ollama.
    OllamaLoadModel(String),

    // GUI_HOOK: Settings → OllamaSection → StartAgent button — show "Agent running" status
    /// User clicked "Start Agent" — spawn/resume the local AI agent worker.
    OllamaStartAgent { model: String },

    // ── Media Review / Culling ─────────────────────────────────────────────
    // GUI_HOOK: MediaIngest → CullingView — fire-and-forget from star/flag/color controls
    /// Set rating/pick/color for a file (fire-and-forget from GUI).
    SetMediaReview {
        file_id:     i64,
        rating:      Option<u8>,
        pick_flag:   Option<String>,
        color_label: Option<String>,
    },

    // GUI_HOOK: MediaIngest → CullingView → file card — populate rating stars, pick flag, color dot
    /// Response after loading review state for a file.
    MediaReviewLoaded {
        file_id:     i64,
        rating:      Option<u8>,
        pick_flag:   String,
        color_label: Option<String>,
        reviewed_at: i64,
    },

    // GUI_HOOK: MediaIngest → CullingView — bulk-populate all visible cards' review states
    /// Bulk review state loaded for multiple files (file_id → (rating, pick_flag, color_label)).
    MediaReviewBulkLoaded(std::collections::HashMap<i64, (Option<u8>, String, Option<String>)>),

    // ── Media Care Queue ────────────────────────────────────────────────────
    // GUI_HOOK: MediaCare screen → JobsPanel — add job row with spinner + item count
    /// A media job was queued.
    MediaJobQueued {
        job_id: String,
        kind: String,
        total_items: usize,
    },

    // GUI_HOOK: MediaCare → JobsPanel → job row — switch to "Running" state with progress bar
    /// A media job started processing.
    MediaJobStarted {
        job_id: String,
    },

    // GUI_HOOK: MediaCare → JobsPanel → job row — update progress bar (done/total, current filename)
    /// Progress update from media worker.
    MediaJobProgress {
        job_id: String,
        done: usize,
        total: usize,
        current_file: String,
    },

    // GUI_HOOK: MediaCare → JobsPanel → job row — mark ✓ DONE; list output paths with "Open" links
    /// A media job completed successfully.
    MediaJobComplete {
        job_id: String,
        outputs: Vec<String>,
    },

    // GUI_HOOK: MediaCare → JobsPanel → job row — mark ✗ FAILED with error message
    /// A media job failed.
    MediaJobFailed {
        job_id: String,
        error: String,
    },

    // ── Phase 4: Symbiosis & Security ──────────────────────────────────────
    // GUI_HOOK: App-wide — change accent color (GREEN / AMBER / RED); show lock overlay on AFK
    /// Security stance changed (Active, AFK, HVT)
    SecurityStanceChanged(SecurityStance),

    // GUI_HOOK: CommandPalette → results list — show interpreted intent as structured action card
    /// Intent interpreted by Fabric
    FabricIntentInterpreted {
        raw_input: String,
        intent:    FabricIntent,
    },

    // GUI_HOOK: CommandPalette → show inline error below input field
    /// Fabric interpretation failed
    FabricFailed(String),

    // GUI_HOOK: CommandPalette / StatusBar → show "Task dispatched to swarm" confirmation chip
    /// Task accepted by Ruflo swarm
    RufloTaskAccepted {
        task_id: String,
        intent:  FabricIntent,
    },

    // GUI_HOOK: CommandPalette / StatusBar → show "Swarm task failed" error toast
    /// Ruflo task failed
    RufloTaskFailed {
        task_id: String,
        error:   String,
    },

    // ── Agents (Sentinel / Librarian / Courier) ────────────────────────────
    // GUI_HOOK: Dashboard → AlertsWidget — prepend alert row with level icon (ℹ / ⚠ / ☢)
    /// Sentinel raised an alert (anomaly, stance change, auth failure, etc.)
    AgentAlert {
        agent:   String,  // "sentinel" | "librarian" | "courier"
        level:   String,  // "info" | "warn" | "critical"
        message: String,
    },

    // GUI_HOOK: Search panel → merge peer results into results list with device_id badge per row
    /// Librarian finished a distributed semantic search.
    LibrarianSearchResult {
        query:   String,
        results: Vec<(i64, f32)>,
        peer_results: std::collections::HashMap<String, Vec<(i64, f32)>>,
    },

    // GUI_HOOK: FileManager / Dashboard → CourierProgress bar (bytes_done / bytes_total)
    /// Courier is transferring a large file (VerteX protocol progress).
    CourierProgress {
        transfer_id: String,
        file_name:   String,
        bytes_done:  u64,
        bytes_total: u64,
        peer_device: String,
    },

    // GUI_HOOK: FileManager → TransferQueue — mark row ✓ COMPLETE; show "Open" link
    /// Courier transfer completed.
    CourierComplete {
        transfer_id: String,
        file_name:   String,
        peer_device: String,
    },

    // GUI_HOOK: FileManager → TransferQueue — mark row ✗ FAILED with error
    /// Courier transfer failed.
    CourierFailed {
        transfer_id: String,
        error:       String,
    },

    // ── Tool Health ──────────────────────────────────────────────────────────
    // GUI_HOOK: Settings → ToolHealthPanel — capability tier badge + per-tool status rows
    // GUI_HOOK: Dashboard → NodeCard header — show CapabilityTier chip (T0/T1/T3/T4)
    /// Probed at startup (and on-demand). Drives capability-tier UI badges.
    ToolHealthUpdated(crate::tool_health::ToolHealthReport),
}
