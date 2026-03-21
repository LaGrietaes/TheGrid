// ═══════════════════════════════════════════════════════════════════════════════
// app.rs — TheGridApp  [v0.3 — Phase 3]
//
// CHANGELOG from v0.2:
//   + db: Arc<Mutex<Database>>  — SQLite index, opened at startup
//   + spawn_index_directory()   — full walk on watch-path add
//   + spawn_incremental_index() — fires on every FileSystemChanged event
//   + spawn_search()            — FTS5 query with 300ms keystroke debounce
//   + spawn_get_telemetry()     — polls remote agent /telemetry every 15s
//   + spawn_collect_local_telemetry() — sysinfo for THIS machine
//   + spawn_wol()               — Wake-on-LAN via thegrid_net::WolSentry
//   + spawn_load_timeline()     — loads recent files from SQLite for The Flow
//   + search: SearchPanelState  — FTS5 search overlay (Ctrl+F)
//   + timeline: TimelineState   — temporal view state
//   + telemetry_cache: HashMap  — NodeTelemetry keyed by device_id
//   + index_stats: IndexStats   — live stats fed to search panel header
//   + All new AppEvent variants handled in process_events()
//   + DetailState.telemetry wired
//   + handle_detail_actions(): fetch_telemetry, wake_device, load_timeline
//   + Keyboard: Ctrl+F → open search; Escape → close search (in search view)
//   + DevicesLoaded now registers devices in DB + spawns local telemetry poll
//   + Auto-scan on app start if watch_paths already configured
//
// Threading contract (unchanged):
//   update() NEVER blocks. All DB/net work is in spawned threads.
//   Results come back via AppEvent on self.event_rx.
// ═══════════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{Color32, Context, RichText};

use thegrid_core::{AppEvent, Config, Database, FileWatcher};
use thegrid_core::models::*;
use thegrid_net::{TailscaleClient, RdpLauncher, AgentClient, AgentServer, WolSentry};
use thegrid_net::rdp::RdpResolution;

use crate::theme::Colors;
use crate::views::dashboard::{
    DashTab, DetailState, DetailActions, SettingsState, render_settings_modal,
};
use crate::views::search::SearchPanelState;
use crate::views::setup::SetupState;
use crate::views::timeline::TimelineState;
use crate::views::terminal::TerminalView;

// ─────────────────────────────────────────────────────────────────────────────
// mpsc convenience alias
// ─────────────────────────────────────────────────────────────────────────────
use std::sync::mpsc;

// ─────────────────────────────────────────────────────────────────────────────
// Screen state machine
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Screen { Boot, Setup, Dashboard }

// ─────────────────────────────────────────────────────────────────────────────
// Toast notification
// ─────────────────────────────────────────────────────────────────────────────

struct Toast {
    message:  String,
    color:    Color32,
    created:  std::time::Instant,
    duration: std::time::Duration,
}
impl Toast {
    fn ok(m: impl Into<String>) -> Self {
        Self { message: m.into(), color: Colors::GREEN,
               created: std::time::Instant::now(),
               duration: std::time::Duration::from_secs(3) }
    }
    fn err(m: impl Into<String>) -> Self {
        Self { message: m.into(), color: Colors::RED,
               created: std::time::Instant::now(),
               duration: std::time::Duration::from_secs(5) }
    }
    fn info(m: impl Into<String>) -> Self {
        Self { message: m.into(), color: Colors::CYAN,
               created: std::time::Instant::now(),
               duration: std::time::Duration::from_secs(3) }
    }
    fn is_expired(&self) -> bool { self.created.elapsed() > self.duration }
}

// ─────────────────────────────────────────────────────────────────────────────
// THE GRID App — owns ALL application state
// ─────────────────────────────────────────────────────────────────────────────

pub struct TheGridApp {
    // ── State machine ─────────────────────────────────────────────────────────
    screen: Screen,
    boot_start: std::time::Instant,

    // ── Config ────────────────────────────────────────────────────────────────
    config:   Config,
    setup:    SetupState,
    settings: SettingsState,

    // ── Devices ───────────────────────────────────────────────────────────────
    devices:             Vec<TailscaleDevice>,
    devices_loading:     bool,
    device_filter:       String,
    selected_idx:        Option<usize>,
    tailscale_connected: bool,

    // ── Per-device UI state ───────────────────────────────────────────────────
    active_tab:     DashTab,
    rdp_username:   String,
    rdp_resolution: String,
    is_tg_agent:    bool,

    // ── Clipboard / file transfer ─────────────────────────────────────────────
    clip_out:     String,
    clip_inbox:   Vec<ClipboardEntry>,
    file_queue:   Vec<FileQueueItem>,
    remote_files: Vec<RemoteFile>,
    transfer_log: Vec<TransferLogEntry>,

    // ── Phase 2: Filesystem watcher ───────────────────────────────────────────
    file_watcher: Option<FileWatcher>,
    watch_paths:  Vec<PathBuf>,

    // ── Phase 3: SQLite index ─────────────────────────────────────────────────
    // Arc<Mutex<>> so background threads can lock, write, and release without
    // blocking the GUI thread. update() never holds the lock.
    db:          Arc<Mutex<Database>>,
    index_stats: IndexStats,

    // ── Phase 3: Search ───────────────────────────────────────────────────────
    search:           SearchPanelState,
    // Timestamp of last keypress — used for 300ms debounce
    search_keystroke: Option<std::time::Instant>,

    timeline: TimelineState,

    mesh_sync_last_at: std::time::Instant,

    // --- Phase 4: Semantic AI ---
    semantic_search:    Arc<Mutex<Option<thegrid_ai::SemanticSearch>>>,
    semantic_enabled:   bool,
    semantic_loading:   bool,
    ai_node_detector:   thegrid_ai::AiNodeDetector,
    is_ai_node:         bool,
    embedding_progress: (usize, usize),

    // ── Phase 3: Telemetry cache ──────────────────────────────────────────────
    // key = Tailscale device_id, value = latest NodeTelemetry snapshot
    telemetry_cache: HashMap<String, NodeTelemetry>,
    // When we last polled each device for telemetry (to rate-limit polls)
    telemetry_last_poll: HashMap<String, std::time::Instant>,
    // Whether a local telemetry collection is in flight
    local_telemetry_pending: bool,

    // ── Background event bus ──────────────────────────────────────────────────
    event_tx: mpsc::Sender<AppEvent>,
    event_rx: mpsc::Receiver<AppEvent>,

    // ── UI state ──────────────────────────────────────────────────────────────
    toasts:         Vec<Toast>,
    status_msg:     String,
    local_hostname: String,

    /// New in Node Enhancement: tracks the current directory being browsed on
    /// the SELECTED remote node. Resets when switching nodes.
    current_remote_path: PathBuf,

    /// New in Node Enhancement: tracks the model name being typed in the UI
    remote_model_edit: String,

    /// New in Node Enhancement: tracks active terminal sessions
    terminal_sessions: HashMap<String, TerminalView>,
}

impl TheGridApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel::<AppEvent>();
        let config   = Config::load().unwrap_or_default();

        let local_hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_uppercase())
            .unwrap_or_else(|_| "UNKNOWN".to_string());

        let setup = SetupState {
            api_key:     config.api_key.clone(),
            device_name: config.device_name.clone(),
            rdp_user:    config.rdp_username.clone(),
            ..Default::default()
        };
        let settings     = SettingsState::from_config(&config);
        let rdp_username = config.rdp_username.clone();

        // ── Open SQLite database ───────────────────────────────────────────────
        let db_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("thegrid")
            .join("index.db");

        let db = match Database::open(&db_path) {
            Ok(d) => {
                log::info!("Database opened at {:?}", db_path);
                Arc::new(Mutex::new(d))
            }
            Err(e) => {
                log::error!("Failed to open database: {} — using in-memory fallback", e);
                // Fallback: in-memory DB (no persistence, but app still runs)
                Arc::new(Mutex::new(
                    Database::open(":memory:").expect("In-memory DB must always work")
                ))
            }
        };

        // ── Start agent server ────────────────────────────────────────────────
        let transfers_dir = config.effective_transfers_dir();
        let mut server = AgentServer::new(
            config.agent_port,
            config.api_key.clone(),
            transfers_dir.clone(),
            tx.clone(),
            config.clone()
        );

        if !config.api_key.trim().is_empty() {
            if let Ok(ts_client) = TailscaleClient::new(config.api_key.clone()) {
                server = server.with_tailscale(Arc::new(ts_client));
            }
        }
        server.spawn();

        // ── Start filesystem watcher ──────────────────────────────────────────
        let mut file_watcher = match FileWatcher::new(tx.clone()) {
            Ok(fw) => { log::info!("FileWatcher ready"); Some(fw) }
            Err(e) => { log::warn!("FileWatcher unavailable: {}", e); None }
        };

        // Initialize with default watch paths from config
        if let Some(fw) = &mut file_watcher {
            for path in &config.watch_paths {
                let _ = fw.watch(path.clone());
            }
        }

        let watch_paths = config.watch_paths.clone();

        let mut app = Self {
            screen:      Screen::Boot,
            boot_start:  std::time::Instant::now(),
            config,
            setup,
            settings,
            devices: Vec::new(),
            devices_loading: false,
            device_filter: String::new(),
            selected_idx: None,
            tailscale_connected: false,
            active_tab:     DashTab::default(),
            rdp_username,
            rdp_resolution: "FULLSCREEN".into(),
            is_tg_agent: false,
            clip_out:   String::new(),
            clip_inbox: Vec::new(),
            file_queue:   Vec::new(),
            remote_files: Vec::new(),
            transfer_log: Vec::new(),
            file_watcher,
            watch_paths,
            db,
            index_stats:  IndexStats::default(),
            search:           SearchPanelState::default(),
            search_keystroke: None,
            timeline: TimelineState::default(),
            telemetry_cache:     HashMap::new(),
            telemetry_last_poll: HashMap::new(),
            local_telemetry_pending: false,
            event_tx: tx,
            event_rx: rx,
            toasts: Vec::new(),
            status_msg: "READY".into(),
            mesh_sync_last_at: std::time::Instant::now(),
            
            // --- Phase 4: AI ---
            semantic_search:   Arc::new(Mutex::new(None)),
            semantic_enabled:  false,
            semantic_loading:  true,
            ai_node_detector:  thegrid_ai::AiNodeDetector::new(),
            is_ai_node:        false,
            embedding_progress: (0, 0),
            local_hostname,
            current_remote_path: PathBuf::new(),
            remote_model_edit: String::new(),
            terminal_sessions: HashMap::new(),
        };
        
        // --- Detect and Start AI ---
        app.is_ai_node = app.ai_node_detector.is_ai_node();
        if app.is_ai_node {
            app.spawn_semantic_initializer();
        } else {
            app.semantic_loading = false;
        }
        
        app
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Background task spawners
    // ─────────────────────────────────────────────────────────────────────────

    fn spawn_load_devices(&mut self) {
        if self.devices_loading { return; }
        self.devices_loading = true;
        self.set_status("Fetching devices from Tailscale...");
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = TailscaleClient::new(api_key).and_then(|c| c.fetch_devices());
            match result {
                Ok(d)  => { let _ = tx.send(AppEvent::DevicesLoaded(d)); }
                Err(e) => { let _ = tx.send(AppEvent::DevicesFailed(e.to_string())); }
            }
        });
    }

    fn spawn_setup_connect(&mut self) {
        self.setup.loading = true;
        self.setup.error = None;
        let api_key = self.setup.api_key.trim().to_string();
        let name    = {
            let n = self.setup.device_name.trim().to_string();
            if n.is_empty() { self.local_hostname.clone() } else { n }
        };
        let rdp = self.setup.rdp_user.trim().to_string();
        let tx  = self.event_tx.clone();
        std::thread::spawn(move || {
            match TailscaleClient::new(&api_key).and_then(|c| c.fetch_devices()) {
                Err(e) => { let _ = tx.send(AppEvent::SetupFailed(e.to_string())); }
                Ok(_)  => {
                    let _ = tx.send(AppEvent::SetupComplete(Config {
                        api_key, device_name: name, rdp_username: rdp,
                        ..Default::default()
                    }));
                }
            }
        });
    }

    fn spawn_ping(&self, ip: String) {
        let port = self.config.agent_port;
        let tx   = self.event_tx.clone();
        let api_key = self.config.api_key.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.ping()) {
                Ok(r)  => { let _ = tx.send(AppEvent::AgentPingOk(r)); }
                Err(e) => { let _ = tx.send(AppEvent::AgentPingFailed(e.to_string())); }
            }
        });
    }

    fn spawn_send_clipboard(&self, ip: String, content: String) {
        let port   = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let sender = self.config.device_name.clone();
        let tx     = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.send_clipboard(&content, &sender)) {
                Ok(_)  => { let _ = tx.send(AppEvent::ClipboardSent); }
                Err(e) => { let _ = tx.send(AppEvent::ClipboardSendFailed(e.to_string())); }
            }
        });
    }

    fn spawn_list_remote_files(&self, ip: String) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx   = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.list_files()) {
                Ok(f)  => { let _ = tx.send(AppEvent::RemoteFilesLoaded(f)); }
                Err(e) => { let _ = tx.send(AppEvent::RemoteFilesFailed(e.to_string())); }
            }
        });
    }

    fn spawn_send_file(&self, ip: String, path: PathBuf, queue_idx: usize) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let _sender = self.config.device_name.clone();
        let tx     = self.event_tx.clone();
        std::thread::spawn(move || {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.upload_file(&path)) {
                Ok(_)  => { let _ = tx.send(AppEvent::FileSent { queue_idx, name }); }
                Err(e) => { let _ = tx.send(AppEvent::FileSendFailed { queue_idx, error: e.to_string() }); }
            }
        });
    }

    fn spawn_download_file(&self, ip: String, filename: String) {
        let port     = self.config.agent_port;
        let api_key  = self.config.api_key.clone();
        let dest_dir = self.config.effective_transfers_dir();
        let tx       = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.download_file(&filename, &dest_dir)) {
                Ok(p)  => { let _ = tx.send(AppEvent::FileDownloaded { name: filename, path: p }); }
                Err(e) => { let _ = tx.send(AppEvent::FileDownloadFailed { name: filename, error: e.to_string() }); }
            }
        });
    }

    fn spawn_browse_remote_directory(&self, ip: String, device_id: String, path: PathBuf) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.browse_directory(&path)) {
                Ok(files) => { let _ = tx.send(AppEvent::RemoteBrowseLoaded { device_id, path, files }); }
                Err(e) => { let _ = tx.send(AppEvent::RemoteBrowseFailed { device_id, error: e.to_string() }); }
            }
        });
    }

    fn spawn_download_remote_file_anywhere(&self, ip: String, path: PathBuf) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let dest_dir = self.config.effective_transfers_dir();
        let tx = self.event_tx.clone();
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "download".to_string());
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.download_remote_file(&path, &dest_dir)) {
                Ok(p) => { let _ = tx.send(AppEvent::FileDownloaded { name, path: p }); }
                Err(e) => { let _ = tx.send(AppEvent::FileDownloadFailed { name, error: e.to_string() }); }
            }
        });
    }

    fn spawn_update_remote_config(&self, ip: String, device_id: String, model: Option<String>, url: Option<String>) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.update_config(model, url)) {
                Ok(_) => { let _ = tx.send(AppEvent::RemoteConfigUpdated { device_id }); }
                Err(e) => { let _ = tx.send(AppEvent::RemoteConfigFailed { device_id, error: e.to_string() }); }
            }
        });
    }

    // ── Phase 3: Index spawners ───────────────────────────────────────────────

    /// Kick off a full directory walk for a newly added watch path.
    /// Sends IndexProgress events during the walk, then IndexComplete.
    fn spawn_index_directory(&self, path: PathBuf, device_id: String, device_name: String) {
        let db = Arc::clone(&self.db);
        let tx = self.event_tx.clone();
        self.set_status_clone(format!("Indexing {}...", path.display()));

        std::thread::Builder::new()
            .name(format!("tg-index-{}", path.display()))
            .spawn(move || {
                let start = std::time::Instant::now();
                let result = {
                    // Lock only for the duration of the index_directory call.
                    // The progress callback fires INSIDE the lock — each call
                    // is brief (just an INSERT/UPSERT) so UI is not starved.
                    match db.lock() {
                        Err(_) => { let _ = tx.send(AppEvent::Status("Index lock failed".into())); return; }
                        Ok(guard) => {
                            guard.index_directory(
                                &device_id,
                                &device_name,
                                &path,
                                |scanned, current| {
                                    // Send progress every 250 files to avoid flooding the channel
                                    if scanned % 250 == 0 {
                                        let current_str = current.file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string();
                                        let _ = tx.send(AppEvent::IndexProgress {
                                            scanned,
                                            total: scanned + 1, // estimated
                                            current: current_str,
                                        });
                                    }
                                }
                            )
                        }
                    }
                };

                match result {
                    Ok(count) => {
                        let elapsed_ms = start.elapsed().as_millis() as u64;
                        let _ = tx.send(AppEvent::IndexComplete {
                            device_id,
                            files_added: count,
                            duration_ms: elapsed_ms,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::Status(format!("Index error: {}", e)));
                    }
                }
            })
            .expect("spawning index thread");
    }

    /// Incrementally re-index a set of changed paths (from FileSystemChanged).
    fn spawn_incremental_index(&self, paths: Vec<PathBuf>) {
        let db          = Arc::clone(&self.db);
        let device_id   = self.config.device_name.clone();
        let device_name = self.config.device_name.clone();
        let tx          = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = db.lock().map(|guard| {
                guard.index_changed_paths(&device_id, &device_name, &paths)
            });
            match result {
                Ok(Ok((updated, _deleted))) => {
                    let _ = tx.send(AppEvent::IndexUpdated { paths_updated: updated });
                }
                Ok(Err(e)) => {
                    log::warn!("Incremental index error: {}", e);
                }
                Err(_) => {
                    log::warn!("DB lock poisoned during incremental index");
                }
            }
        });
    }

    /// Run an FTS5 search. Generation counter prevents stale results overwriting
    /// newer ones if multiple searches are in flight simultaneously.
    /// Sync a single remote node's index delta. (Phase 3)
    fn spawn_sync_node(&self, device_id: String, ip: String, hostname: String) {
        let db          = self.db.clone();
        let event_tx    = self.event_tx.clone();
        let api_key     = self.config.api_key.clone();
        let port        = self.config.agent_port;

        std::thread::spawn(move || {
            let last_ts = match db.lock() {
                Ok(guard) => guard.get_node_sync_ts(&device_id).unwrap_or(0),
                Err(_)    => 0,
            };

            let client = match AgentClient::new(&ip, port, api_key) {
                Ok(c) => c,
                Err(e) => {
                    log::debug!("Sync connection failed for {}: {}", device_id, e);
                    return;
                }
            };

            match client.sync_index(last_ts) {
                Ok(results) => {
                    let mut count = 0;
                    let mut max_ts = last_ts;
                    if let Ok(guard) = db.lock() {
                        for r in results {
                            let mod_ts = r.modified.unwrap_or(0);
                            if mod_ts > max_ts { max_ts = mod_ts; }
                            if guard.upsert_remote_file(r).is_ok() {
                                count += 1;
                            }
                        }
                        let _ = guard.update_node_sync_ts(&device_id, &hostname, max_ts);
                    }
                    if count > 0 {
                        let _ = event_tx.send(AppEvent::SyncComplete { device_id, files_added: count });
                    }
                }
                Err(e) => {
                    log::debug!("Sync failed for {}: {}", device_id, e);
                }
            }
        });
    }

    /// Pull index deltas from ALL reachable Tailscale nodes. (Phase 3)
    fn sync_all_nodes(&self) {
        log::debug!("Initiating mesh index synchronization...");
        for device in &self.devices {
            if device.name == self.config.device_name { continue; }
            if let Some(ip) = device.primary_ip() {
                self.spawn_sync_node(device.id.clone(), ip.to_string(), device.hostname.clone());
            }
        }
    }

    /// Initialize the semantic search engine in a background thread.
    fn spawn_semantic_initializer(&self) {
        let event_tx = self.event_tx.clone();
        let db       = self.db.clone();
        let semantic = self.semantic_search.clone();
        
        std::thread::spawn(move || {
            match thegrid_ai::FastEmbedProvider::new() {
                Ok(provider) => {
                    let provider: Arc<dyn thegrid_ai::EmbeddingProvider> = Arc::new(provider);
                    match thegrid_ai::SemanticSearch::new(provider) {
                        Ok(mut search) => {
                            // Pre-load all existing embeddings from DB into the USearch index
                            if let Ok(guard) = db.lock() {
                                if let Ok(all) = guard.get_all_embeddings() {
                                    for (fid, vec) in all {
                                        let _ = search.add_vector(fid, &vec);
                                    }
                                }
                            }
                            
                            // Store the initialized engine
                            {
                                let mut lock = semantic.lock().expect("semantic lock");
                                *lock = Some(search);
                            }
                            
                            let _ = event_tx.send(AppEvent::SemanticReady);
                        }
                        Err(e) => {
                            let _ = event_tx.send(AppEvent::SemanticFailed(format!("Vector engine: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    let _ = event_tx.send(AppEvent::SemanticFailed(format!("Model download: {}", e)));
                }
            }
        });
    }

    /// Background worker that processes files and generates embeddings.
    fn spawn_embedding_worker(&self) {
        let db       = self.db.clone();
        let event_tx = self.event_tx.clone();
        let semantic = self.semantic_search.clone();
        
        std::thread::spawn(move || {
            let total = db.lock().ok().and_then(|g| g.count_unindexed_files().ok()).unwrap_or(0);
            let mut indexed = 0;

            loop {
                // Check if engine is still there
                let has_engine = {
                    let lock = semantic.lock().expect("semantic lock");
                    lock.is_some()
                };
                if !has_engine { break; }

                let batch = match db.lock() {
                    Ok(guard) => guard.get_files_needing_embedding(20).unwrap_or_default(),
                    Err(_) => break,
                };
                
                if batch.is_empty() { break; }
                
                for (fid, text) in batch {
                    let mut lock = semantic.lock().expect("semantic lock");
                    if let Some(search) = &mut *lock {
                        if let Ok(vec) = search.index_file(fid, &text) {
                            if let Ok(db_lock) = db.lock() {
                                let _ = db_lock.save_embedding(fid, search.model_id(), &vec);
                            }
                        }
                    }
                    indexed += 1;
                    let _ = event_tx.send(AppEvent::EmbeddingProgress { indexed, total });
                }
                
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    }

    fn spawn_search(&mut self) {
        let query     = self.search.query.trim().to_string();
        if query.is_empty() {
            self.search.results.clear();
            self.search.searching = false;
            return;
        }

        let gen      = self.search.mark_dispatched();
        let db       = Arc::clone(&self.db);
        let tx       = self.event_tx.clone();
        let dev_filt = self.search.device_filter.clone();
        let semantic_enabled = self.semantic_enabled;
        let semantic_search  = self.semantic_search.clone();

        std::thread::spawn(move || {
            let results = if semantic_enabled {
                if let Ok(mut lock) = semantic_search.lock() {
                    if let Some(engine) = &mut *lock {
                        match engine.search(&query, 50) {
                            Ok(hits) => {
                                let ids: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
                                db.lock().ok().and_then(|guard| guard.get_files_by_ids(&ids).ok()).unwrap_or_default()
                            }
                            Err(e) => {
                                log::error!("Semantic search failed: {}", e);
                                vec![]
                            }
                        }
                    } else { vec![] }
                } else { vec![] }
            } else {
                db.lock().ok().and_then(|guard| {
                    guard.search_fts(&query, 50, dev_filt.as_deref()).ok()
                }).unwrap_or_default()
            };

            let _ = tx.send(AppEvent::SearchResults(results));
            let _ = tx.send(AppEvent::Status(format!("search_gen:{}", gen)));
        });
    }

    /// Fetch hardware telemetry from a remote agent (rate-limited to once per 15s).
    fn spawn_get_telemetry(&mut self, ip: String, device_id: String) {
        let now = std::time::Instant::now();
        if let Some(&last) = self.telemetry_last_poll.get(&device_id) {
            if last.elapsed().as_secs() < 15 { return; }
        }
        self.telemetry_last_poll.insert(device_id.clone(), now);

        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx   = self.event_tx.clone();
        std::thread::spawn(move || {
            if let Ok(client) = AgentClient::new(&ip, port, api_key) {
                if let Ok(telemetry) = client.get_telemetry() {
                    let _ = tx.send(AppEvent::TelemetryUpdate { device_id, telemetry });
                }
            }
        });
    }

    /// Collect telemetry for the LOCAL machine via sysinfo (non-blocking).
    fn spawn_collect_local_telemetry(&mut self) {
        if self.local_telemetry_pending { return; }
        self.local_telemetry_pending = true;

        let device_id = self.config.device_name.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let telemetry = crate::telemetry::collect_local();
            let _ = tx.send(AppEvent::TelemetryUpdate { device_id, telemetry });
        });
    }

    /// Send a Wake-on-LAN magic packet.
    /// `mac_addr` format: "AA:BB:CC:DD:EE:FF"
    fn spawn_wol(&mut self, device_name: String, mac_addr: String) {
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            // Broadcast on both the standard broadcast and the Tailscale subnet broadcast
            let result = WolSentry::send_multi(
                &mac_addr,
                &["255.255.255.255", "100.64.0.255"],
            );
            match result {
                Ok(_) => {
                    let _ = tx.send(AppEvent::WolSent {
                        device_name,
                        target_mac: mac_addr,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::WolFailed { reason: e.to_string() });
                }
            }
        });
    }

    /// Load recent files from SQLite for the Timeline view.
    fn spawn_load_timeline(&mut self) {
        if self.timeline.loading { return; }
        self.timeline.loading = true;

        let db   = Arc::clone(&self.db);
        let tx   = self.event_tx.clone();
        let filt = self.timeline.device_filter.clone();
        std::thread::spawn(move || {
            let result = db.lock().map(|guard| {
                guard.get_recent_files(200, filt.as_deref())
            });
            match result {
                Ok(Ok(entries)) => { let _ = tx.send(AppEvent::TemporalLoaded(entries)); }
                Ok(Err(e))      => { log::warn!("Timeline load error: {}", e); }
                Err(_)          => {}
            }
        });
    }

    /// Update index_stats from the DB (cheap count query, safe to call often).
    fn refresh_index_stats(&mut self) {
        let db = Arc::clone(&self.db);
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            if let Ok(guard) = db.lock() {
                let total = guard.file_count(None).unwrap_or(0);
                let _ = tx.send(AppEvent::Status(format!("index_count:{}", total)));
            }
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Filesystem watcher + watch path management
    // ─────────────────────────────────────────────────────────────────────────

    fn add_watch_directory(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Select directory to watch")
            .pick_folder()
        {
            if self.watch_paths.contains(&path) {
                self.push_toast(Toast::info("Already watching that directory"));
                return;
            }
            match &mut self.file_watcher {
                None => {
                    self.push_toast(Toast::err("File watcher unavailable"));
                }
                Some(fw) => {
                    match fw.watch(path.clone()) {
                        Ok(_) => {
                            let label = path.file_name()
                                .unwrap_or_default().to_string_lossy().to_string();
                            self.watch_paths.push(path.clone());
                            self.push_toast(Toast::ok(format!("Watching: {}", label)));

                            // Kick off a full index scan for the new path
                            let dev_id   = self.config.device_name.clone();
                            let dev_name = self.config.device_name.clone();
                            self.spawn_index_directory(path, dev_id, dev_name);
                        }
                        Err(e) => {
                            self.push_toast(Toast::err(format!("Watch failed: {}", e)));
                        }
                    }
                }
            }
        }
    }

    // Helper: set_status from a non-&mut self context (used in spawner closures)
    fn set_status_clone(&self, msg: String) {
        let _ = self.event_tx.send(AppEvent::Status(msg));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Event processor — drains mpsc channel every frame
    // ─────────────────────────────────────────────────────────────────────────

    fn process_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {

                // ── Devices ───────────────────────────────────────────────────
                AppEvent::DevicesLoaded(devices) => {
                    self.devices_loading     = false;
                    self.tailscale_connected = true;
                    let n = devices.len();

                    // Register all devices in the DB so names are available offline
                    {
                        if let Ok(guard) = self.db.lock() {
                            for d in &devices {
                                let _ = guard.upsert_device(&d.id, d.display_name());
                            }
                        }
                    }

                    self.devices = devices;
                    self.set_status(format!("{} nodes discovered", n));

                    // Start local telemetry collection immediately after first load
                    self.spawn_collect_local_telemetry();
                }

                AppEvent::DevicesFailed(err) => {
                    self.devices_loading     = false;
                    self.tailscale_connected = false;
                    self.push_toast(Toast::err(format!("Tailscale: {}", err)));
                    self.set_status(format!("Connection failed: {}", err));
                }

                // ── Setup / config ────────────────────────────────────────────
                AppEvent::SetupComplete(config) => {
                    self.setup.loading = false;
                    if let Err(e) = config.save() {
                        log::error!("Config save failed: {}", e);
                    }
                    self.rdp_username = config.rdp_username.clone();
                    self.settings     = SettingsState::from_config(&config);
                    self.config       = config;
                    self.screen       = Screen::Dashboard;
                    self.spawn_load_devices();
                }

                AppEvent::SetupFailed(err) => {
                    self.setup.loading = false;
                    self.setup.error   = Some(err);
                }

                // ── Agent ─────────────────────────────────────────────────────
                AppEvent::AgentPingOk(resp) => {
                    self.is_tg_agent = true;
                    self.push_toast(Toast::ok(format!("⬡ Agent online: {}", resp.device)));
                }
                AppEvent::AgentPingFailed(err) => {
                    self.is_tg_agent = false;
                    self.push_toast(Toast::err(format!("Agent ping failed: {}", err)));
                }

                // ── File transfer ─────────────────────────────────────────────
                AppEvent::RemoteFilesLoaded(files) => {
                    let n = files.len();
                    self.remote_files = files;
                    self.set_status(format!("{} remote files", n));
                }
                AppEvent::RemoteFilesFailed(err) => {
                    self.push_toast(Toast::err(format!("File scan: {}", err)));
                }

                AppEvent::RemoteBrowseLoaded { device_id, path, files } => {
                    if self.selected_idx.and_then(|i| self.devices.get(i)).map(|d| d.id == device_id).unwrap_or(false) {
                        self.remote_files = files;
                        self.current_remote_path = path;
                        self.set_status("Remote directory loaded");
                    }
                }
                AppEvent::RemoteBrowseFailed { device_id: _, error } => {
                    self.push_toast(Toast::err(format!("Browse failed: {}", error)));
                }

                AppEvent::RemoteConfigUpdated { device_id: _ } => {
                    self.push_toast(Toast::ok("Remote configuration updated"));
                }
                AppEvent::RemoteConfigFailed { device_id: _, error } => {
                    self.push_toast(Toast::err(format!("Config update failed: {}", error)));
                }
                AppEvent::FileSent { queue_idx, name } => {
                    if let Some(item) = self.file_queue.get_mut(queue_idx) {
                        item.status = FileTransferStatus::Done;
                    }
                    self.transfer_log.push(TransferLogEntry::ok(format!("✓ Sent: {}", name)));
                    self.push_toast(Toast::ok(format!("Sent: {}", name)));
                }
                AppEvent::FileSendFailed { queue_idx, error } => {
                    if let Some(item) = self.file_queue.get_mut(queue_idx) {
                        item.status = FileTransferStatus::Failed(error.clone());
                    }
                    self.transfer_log.push(TransferLogEntry::err(format!("✗ {}", error)));
                    self.push_toast(Toast::err(format!("Send failed: {}", error)));
                }
                AppEvent::FileDownloadFailed { name, error } => {
                    self.transfer_log.push(TransferLogEntry::err(
                        format!("✗ {}: {}", name, error)
                    ));
                    self.push_toast(Toast::err(format!("Download failed: {}", name)));
                }

                // ── Remote Terminal ───────────────────────────────────────────
                AppEvent::RemoteTerminalCreated { device_id, session_id } => {
                    self.terminal_sessions.insert(device_id.clone(), TerminalView::new());
                    self.push_toast(Toast::ok("Terminal session established"));
                    // Start polling for output
                    self.spawn_poll_terminal_output(device_id, session_id);
                }
                AppEvent::RemoteTerminalFailed { device_id: _, error } => {
                    self.push_toast(Toast::err(format!("Terminal failed: {}", error)));
                }
                AppEvent::RemoteTerminalOutput { device_id, data } => {
                    if let Some(view) = self.terminal_sessions.get_mut(&device_id) {
                        view.push_output(&data);
                    }
                }

                // ── Clipboard ─────────────────────────────────────────────────
                AppEvent::ClipboardSent => {
                    self.push_toast(Toast::ok("Clipboard transmitted!"));
                }
                AppEvent::ClipboardSendFailed(err) => {
                    self.push_toast(Toast::err(format!("Clipboard: {}", err)));
                }
                AppEvent::ClipboardReceived(entry) => {
                    self.push_toast(Toast::info(format!("Clipboard from {}", entry.sender)));
                    self.clip_inbox.push(entry);
                }
                AppEvent::FileReceived { name, size: _ } => {
                    self.transfer_log.push(TransferLogEntry::ok(format!("⬇ Received: {}", name)));
                    self.push_toast(Toast::ok(format!("Received: {}", name)));
                }

                // ── Filesystem watcher (Phase 2) ──────────────────────────────
                AppEvent::FileSystemChanged { paths, summary } => {
                    self.set_status(format!("⬡ {}", summary));
                    // Phase 3: trigger incremental index update
                    self.spawn_incremental_index(paths);
                    // Refresh timeline if it's visible
                    if self.active_tab == DashTab::Timeline {
                        self.spawn_load_timeline();
                    }
                }
                AppEvent::FileWatcherError(err) => {
                    log::error!("FileWatcher: {}", err);
                    self.push_toast(Toast::err(format!("Watcher error: {}", err)));
                }

                // ── Phase 3: Mesh Sync ─────────────────────────────────────────
                AppEvent::SyncRequest { after, response_tx } => {
                    let db = self.db.clone();
                    std::thread::spawn(move || {
                        if let Ok(guard) = db.lock() {
                            let results = guard.get_files_after(after).unwrap_or_default();
                            let _ = response_tx.send(results);
                        }
                    });
                }

                AppEvent::SyncComplete { device_id, files_added } => {
                    log::info!("Sync complete for {}: {} items", device_id, files_added);
                    self.refresh_index_stats();
                }
                AppEvent::SyncFailed { device_id, error } => {
                    log::debug!("Sync failed for {}: {}", device_id, error);
                }

                // ── Phase 4: Semantic ─────────────────────────────────────────
                AppEvent::SemanticReady => {
                    log::info!("Semantic search engine is ready");
                    self.semantic_loading = false;
                    // Trigger first indexing pass
                    self.spawn_embedding_worker();
                }
                AppEvent::EmbeddingProgress { indexed, total } => {
                    self.embedding_progress = (indexed, total);
                }

                AppEvent::SemanticFailed(err) => {
                    log::error!("Semantic AI failure: {}", err);
                    self.semantic_loading = false;
                    self.push_toast(Toast::err(format!("AI failed: {}", err)));
                }

                // ── Phase 3: Index ────────────────────────────────────────────
                AppEvent::IndexProgress { scanned, total, current } => {
                    self.index_stats.scanning = true;
                    self.index_stats.scan_progress = scanned;
                    self.index_stats.scan_total    = total;
                    self.set_status(format!("Indexing… {} files  ({})", scanned, current));
                }
                AppEvent::IndexComplete { device_id: _, files_added, duration_ms } => {
                    self.index_stats.scanning    = false;
                    self.index_stats.total_files += files_added;
                    self.index_stats.last_scanned = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64
                    );
                    self.set_status(format!(
                        "Index complete: {} files in {:.1}s",
                        files_added, duration_ms as f64 / 1000.0
                    ));
                    self.push_toast(Toast::ok(format!(
                        "Indexed {} files", files_added
                    )));
                    self.refresh_index_stats();

                    // Phase 4: Trigger embedding generation for the new files
                    if !self.semantic_loading && self.is_ai_node {
                        self.spawn_embedding_worker();
                    }
                }
                AppEvent::IndexUpdated { paths_updated } => {
                    if paths_updated > 0 {
                        self.set_status(format!("⬡ Incremental index: {} items updated", paths_updated));
                        self.refresh_index_stats();

                        // Phase 4: Trigger embedding generation for the changes
                        if !self.semantic_loading && self.is_ai_node {
                            self.spawn_embedding_worker();
                        }
                    }
                }

                // ── Phase 3: Search ───────────────────────────────────────────
                AppEvent::SearchResults(results) => {
                    // Generation-tagged results arrive via Status("search_gen:N")
                    // before SearchResults — handled below. Accept all results for now.
                    self.search.receive_results(self.search.query_gen, results);
                }

                // ── Phase 3: Telemetry ────────────────────────────────────────
                AppEvent::TelemetryUpdate { device_id, telemetry } => {
                    // Mark local telemetry as no longer pending
                    if device_id == self.config.device_name {
                        self.local_telemetry_pending = false;
                    }
                    self.telemetry_cache.insert(device_id, telemetry);
                }

                // ── Phase 3: WoL ──────────────────────────────────────────────
                AppEvent::WolSent { device_name, target_mac: _ } => {
                    self.push_toast(Toast::ok(format!("⚡ Wake packet sent to {}", device_name)));
                }
                AppEvent::WolFailed { reason } => {
                    self.push_toast(Toast::err(format!("WoL failed: {}", reason)));
                }

                // ── Phase 3: Timeline ─────────────────────────────────────────
                AppEvent::TemporalLoaded(entries) => {
                    self.timeline.entries = entries;
                    self.timeline.mark_refreshed();
                }

                // ── UI / misc ─────────────────────────────────────────────────
                AppEvent::Status(msg) => {
                    // Special-cased status messages used as piggyback channels
                    if msg.starts_with("index_count:") {
                        if let Ok(n) = msg["index_count:".len()..].parse::<u64>() {
                            self.index_stats.total_files = n;
                        }
                    } else if !msg.starts_with("search_gen:") {
                        // Regular status messages go to the status bar
                        self.set_status(msg);
                    }
                }
                AppEvent::RequestRefresh => { self.spawn_load_devices(); }
                AppEvent::OpenSettings   => { self.settings.open = true; }
                _ => {}
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // UI helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn push_toast(&mut self, t: Toast) { self.toasts.push(t); }
    fn set_status(&mut self, msg: impl Into<String>) { self.status_msg = msg.into(); }
    fn selected_ip(&self) -> Option<String> {
        self.selected_idx
            .and_then(|i| self.devices.get(i))
            .and_then(|d| d.primary_ip())
            .map(|s| s.to_string())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Titlebar
    // ─────────────────────────────────────────────────────────────────────────

    fn render_titlebar(&self, ctx: &Context) {
        egui::TopBottomPanel::top("titlebar")
            .exact_height(36.0)
            .frame(egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .stroke(egui::Stroke::new(1.0, Colors::BORDER))
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(12.0);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    let c = rect.center();
                    let r = 6.0;
                    let mut points = vec![];
                    for i in 0..6 {
                        let angle = std::f32::consts::PI / 3.0 * i as f32 + std::f32::consts::PI / 2.0;
                        points.push(c + egui::vec2(r * angle.cos(), r * angle.sin()));
                    }
                    ui.painter().add(egui::Shape::convex_polygon(points, Color32::TRANSPARENT, egui::Stroke::new(1.5, Colors::GREEN)));
                    ui.add_space(6.0);
                    ui.label(RichText::new("THE GRID").color(Colors::GREEN).size(11.0).strong());

                    ui.add_space(12.0);
                    let (dot_color, status_text) = if self.tailscale_connected {
                        (Colors::GREEN, "TAILSCALE CONNECTED")
                    } else if self.devices_loading {
                        (Colors::AMBER, "CONNECTING...")
                    } else {
                        (Colors::TEXT_MUTED, "DISCONNECTED")
                    };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.0, dot_color);
                    ui.label(RichText::new(status_text).color(Colors::TEXT_DIM).size(9.0));

                    // Show index count in titlebar
                    if self.index_stats.total_files > 0 {
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(format!("{} indexed", self.index_stats.total_files))
                                .color(Colors::TEXT_MUTED).size(9.0)
                        );
                    }
                    if self.index_stats.scanning {
                        ui.add_space(4.0);
                        ui.spinner();
                    }

                    // Draggable region
                    let drag = ui.interact(
                        ui.available_rect_before_wrap(),
                        egui::Id::new("titlebar_drag"),
                        egui::Sense::drag(),
                    );
                    if drag.dragged() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Close
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), egui::Sense::click());
                        let color = if resp.hovered() { Colors::TEXT } else { Colors::TEXT_DIM };
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, egui::Rounding::ZERO, Color32::from_rgb(180, 20, 40));
                        }
                        let c = rect.center();
                        let e = 4.0;
                        ui.painter().line_segment([c - egui::vec2(e, e), c + egui::vec2(e, e)], egui::Stroke::new(1.2, color));
                        ui.painter().line_segment([c - egui::vec2(e, -e), c + egui::vec2(e, -e)], egui::Stroke::new(1.2, color));
                        if resp.clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }

                        // Maximize
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), egui::Sense::click());
                        let color = if resp.hovered() { Colors::TEXT } else { Colors::TEXT_DIM };
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, egui::Rounding::ZERO, Colors::BORDER2);
                        }
                        let c = rect.center();
                        let e = 4.0;
                        ui.painter().rect_stroke(
                            egui::Rect::from_center_size(c, egui::vec2(e * 2.0, e * 2.0)),
                            egui::Rounding::ZERO,
                            egui::Stroke::new(1.2, color)
                        );
                        if resp.clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true)); }

                        // Minimize
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 24.0), egui::Sense::click());
                        let color = if resp.hovered() { Colors::TEXT } else { Colors::TEXT_DIM };
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, egui::Rounding::ZERO, Colors::BORDER2);
                        }
                        let c = rect.center();
                        let e = 4.5;
                        ui.painter().line_segment([c - egui::vec2(e, -e), c + egui::vec2(e, -e)], egui::Stroke::new(1.2, color));
                        if resp.clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }

                        ui.add_space(8.0);

                        // Settings Gear (Vector)
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                        let color = if resp.hovered() { Colors::TEXT } else { Colors::TEXT_DIM };
                        let c = rect.center();
                        // Draw gear
                        ui.painter().circle_stroke(c, 4.0, egui::Stroke::new(1.2, color));
                        for i in 0..8 {
                            let angle = std::f32::consts::PI / 4.0 * i as f32;
                            let p1 = c + egui::vec2(5.0 * angle.cos(), 5.0 * angle.sin());
                            let p2 = c + egui::vec2(7.0 * angle.cos(), 7.0 * angle.sin());
                            ui.painter().line_segment([p1, p2], egui::Stroke::new(1.2, color));
                        }
                        if resp.clicked() { let _ = self.event_tx.send(AppEvent::OpenSettings); }

                        ui.add_space(4.0);

                        // Search Magnifier (Vector)
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                        let color = if resp.hovered() { Colors::TEXT } else { Colors::TEXT_DIM };
                        let c = rect.center() - egui::vec2(1.0, 1.0);
                        ui.painter().circle_stroke(c, 4.0, egui::Stroke::new(1.2, color));
                        ui.painter().line_segment(
                            [c + egui::vec2(3.0, 3.0), c + egui::vec2(6.0, 6.0)],
                            egui::Stroke::new(1.5, color)
                        );
                        if resp.clicked() { 
                            // Handled via keyboard / event logic
                        }

                        ui.add_space(8.0);
                        ui.label(RichText::new(&self.config.device_name).color(Colors::TEXT_MUTED).size(9.0));
                    });
                });
            });
    }

    fn render_statusbar(&self, ctx: &Context) {
        egui::TopBottomPanel::bottom("statusbar")
            .exact_height(24.0)
            .frame(egui::Frame::none()
                .fill(Colors::BG_PANEL)
                .stroke(egui::Stroke::new(1.0, Colors::BORDER))
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(14.0);
                    ui.label(RichText::new(format!("{} NODES", self.devices.len())).color(Colors::TEXT_MUTED).size(9.0));
                    ui.label(RichText::new("|").color(Colors::BORDER).size(9.0));
                    ui.label(RichText::new("TAILSCALE").color(Colors::TEXT_MUTED).size(9.0));
                    ui.label(RichText::new("|").color(Colors::BORDER).size(9.0));
                    ui.label(RichText::new(format!("AGENT :{}", self.config.agent_port)).color(Colors::TEXT_MUTED).size(9.0));
                    if self.file_watcher.is_some() {
                        ui.label(RichText::new("|").color(Colors::BORDER).size(9.0));
                        ui.label(
                            RichText::new(format!("WATCHING {}", self.watch_paths.len()))
                                .color(if self.watch_paths.is_empty() { Colors::TEXT_MUTED } else { Colors::GREEN })
                                .size(9.0)
                        );
                    }
                    ui.label(RichText::new("|").color(Colors::BORDER).size(9.0));
                    ui.label(
                        RichText::new(format!("{} INDEXED", self.index_stats.total_files))
                            .color(Colors::TEXT_MUTED).size(9.0)
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(14.0);
                        ui.label(RichText::new(&self.status_msg).color(Colors::TEXT_DIM).size(9.0));
                    });
                });
            });
    }

    fn render_toasts(&mut self, ctx: &Context) {
        self.toasts.retain(|t| !t.is_expired());
        let mut y = 46.0_f32;
        for (i, toast) in self.toasts.iter().enumerate() {
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new(("toast", i)))
                .fixed_pos(egui::pos2(screen.max.x - 320.0, screen.min.y + y))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(Colors::BG_PANEL)
                        .stroke(egui::Stroke::new(1.0, Colors::BORDER2))
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            let r = ui.min_rect();
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(r.min.x - 14.0, r.min.y - 10.0),
                                    egui::vec2(3.0, r.height() + 20.0),
                                ),
                                egui::Rounding::ZERO, toast.color,
                            );
                            ui.label(RichText::new(&toast.message).color(Colors::TEXT).size(10.0));
                        });
                });
            y += 44.0;
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Detail actions dispatcher
    // ─────────────────────────────────────────────────────────────────────────

    fn handle_detail_actions(&mut self, actions: DetailActions, ip: &str, device_id: &str) {
        if actions.launch_rdp {
            let user = if self.rdp_username.trim().is_empty() { None }
                       else { Some(self.rdp_username.trim()) };
            let res = RdpResolution::from_str(&self.rdp_resolution);
            match RdpLauncher::launch(ip, user, &res) {
                Ok(_)  => self.push_toast(Toast::ok(format!("RDP → {}", ip))),
                Err(e) => self.push_toast(Toast::err(format!("RDP failed: {}", e))),
            }
        }

        if actions.browse_share {
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("explorer.exe").arg(format!("\\\\{}", ip)).spawn();
            self.push_toast(Toast::info("Opening network share..."));
        }

        if actions.ping {
            self.push_toast(Toast::info(format!("Pinging {}...", ip)));
            self.spawn_ping(ip.to_string());
        }

        if actions.load_clipboard {
            match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                Ok(text) => { self.clip_out = text; self.push_toast(Toast::ok("Clipboard loaded")); }
                Err(e)   => { self.push_toast(Toast::err(format!("Clipboard: {}", e))); }
            }
        }

        if actions.send_clipboard && !self.clip_out.is_empty() {
            self.spawn_send_clipboard(ip.to_string(), self.clip_out.clone());
        }

        if actions.select_files {
            if let Some(paths) = rfd::FileDialog::new().set_title("Select files to send").pick_files() {
                for path in paths {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let idx  = self.file_queue.len();
                    self.file_queue.push(FileQueueItem {
                        path: path.clone(), name: name.clone(), size,
                        status: FileTransferStatus::Sending,
                    });
                    self.transfer_log.push(TransferLogEntry::info(format!("Sending {}...", name)));
                    self.spawn_send_file(ip.to_string(), path, idx);
                }
            }
        }

        if actions.scan_remote {
            self.spawn_list_remote_files(ip.to_string());
        }

        if let Some(path) = actions.browse_remote {
            self.spawn_browse_remote_directory(ip.to_string(), device_id.to_string(), path);
        }

        if let Some(filename) = actions.download_file {
            self.transfer_log.push(TransferLogEntry::info(format!("Downloading {}...", filename)));
            self.spawn_download_file(ip.to_string(), filename);
        }

        if let Some(path) = actions.download_remote_file {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            self.transfer_log.push(TransferLogEntry::info(format!("Downloading {}...", name)));
            self.spawn_download_remote_file_anywhere(ip.to_string(), path);
        }

        if let Some((model, url)) = actions.update_remote_config {
            self.spawn_update_remote_config(ip.to_string(), device_id.to_string(), model, url);
        }

        if actions.create_terminal {
            self.spawn_create_terminal_session(ip.to_string(), device_id.to_string());
        }

        if actions.launch_scrcpy {
            self.spawn_launch_scrcpy(ip.to_string());
        }

        if actions.open_inbox {
            let dir = self.config.effective_transfers_dir();
            let _ = std::fs::create_dir_all(&dir);
            #[cfg(target_os = "windows")] let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
            #[cfg(target_os = "linux")]   let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
            #[cfg(target_os = "macos")]   let _ = std::process::Command::new("open").arg(&dir).spawn();
        }

        if actions.add_watch_path { self.add_watch_directory(); }

        if actions.fetch_telemetry {
            self.spawn_get_telemetry(ip.to_string(), device_id.to_string());
        }

        if actions.wake_device {
            self.push_toast(Toast::info("WoL: enter MAC in settings (feature coming in Phase 4)"));
        }

        if actions.load_timeline {
            self.spawn_load_timeline();
        }
    }

    // ── Remote Terminal Spawners ─────────────────────────────────────────────

    fn spawn_create_terminal_session(&mut self, ip: String, device_id: String) {
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, 3030, api_key) {
                Ok(client) => match client.create_terminal_session() {
                    Ok(session_id) => {
                        let _ = tx.send(AppEvent::RemoteTerminalCreated { device_id, session_id });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::RemoteTerminalFailed { device_id, error: e.to_string() });
                    }
                },
                Err(e) => {
                    let _ = tx.send(AppEvent::RemoteTerminalFailed { device_id, error: e.to_string() });
                }
            }
        });
    }

    fn spawn_poll_terminal_output(&self, device_id: String, session_id: String) {
        let ip = self.devices.iter().find(|d| d.id == device_id).and_then(|d| d.primary_ip().map(|s| s.to_string()));
        let Some(ip) = ip else { return; };
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();

        std::thread::spawn(move || {
            if let Ok(client) = AgentClient::new(&ip, 3030, api_key) {
                loop {
                    match client.get_terminal_output(&session_id) {
                        Ok(data) => {
                            if !data.is_empty() {
                                let _ = tx.send(AppEvent::RemoteTerminalOutput { device_id: device_id.clone(), data });
                            }
                        }
                        Err(_) => break,
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        });
    }

    fn spawn_send_terminal_input(&self, ip: String, session_id: String, data: Vec<u8>) {
        let api_key = self.config.api_key.clone();
        std::thread::spawn(move || {
            if let Ok(client) = AgentClient::new(&ip, 3030, api_key) {
                let _ = client.send_terminal_input(&session_id, &data);
            }
        });
    }

    fn spawn_launch_scrcpy(&self, ip: String) {
        std::thread::spawn(move || {
            let _ = std::process::Command::new("scrcpy")
                .arg(format!("--tcpip={}", ip))
                .spawn();
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// eframe::App — the render loop
// ─────────────────────────────────────────────────────────────────────────────

impl eframe::App for TheGridApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 1. Drain background event channel
        self.process_events();

        // 2. Global keyboard shortcuts
        let ctrl_f       = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::F));
        let ctrl_comma   = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Num0)); // placeholder
        let _ctrl_r      = ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::R));

        if ctrl_f && self.screen == Screen::Dashboard {
            self.search.open = !self.search.open;
        }
        let _ = ctrl_comma;

        // 3. Search debounce: dispatch search 300ms after last keypress
        if self.search.open && self.search.query_changed() {
            self.search_keystroke = Some(std::time::Instant::now());
        }
        if let Some(ks) = self.search_keystroke {
            if ks.elapsed().as_millis() >= 300 && self.search.query_changed() {
                self.spawn_search();
                self.search_keystroke = None;
            }
        }

        // 4. Handle drag-dropped files
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect()
        });
        if !dropped.is_empty() {
            if let Some(ip) = self.selected_ip() {
                for path in dropped {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let idx  = self.file_queue.len();
                    self.file_queue.push(FileQueueItem {
                        path: path.clone(), name: name.clone(), size,
                        status: FileTransferStatus::Sending,
                    });
                    self.transfer_log.push(TransferLogEntry::info(format!("Sending {}...", name)));
                    self.spawn_send_file(ip.clone(), path, idx);
                }
            }
        }

        // 5. Periodic local telemetry refresh (every 30s)
        // We rate-limit in spawn_collect_local_telemetry itself, so this is safe to call often
        if self.screen == Screen::Dashboard
            && !self.local_telemetry_pending
            && self.telemetry_last_poll
                .get(&self.config.device_name)
                .map(|t| t.elapsed().as_secs() > 30)
                .unwrap_or(true)
        {
            self.spawn_collect_local_telemetry();
        }

        // 6. Screen dispatch
        match self.screen.clone() {

            Screen::Boot => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(Colors::BG))
                    .show(ctx, |ui| {
                        let elapsed = self.boot_start.elapsed().as_secs_f32();
                        let done = crate::views::boot::render(ui, elapsed);
                        if done {
                            if self.config.is_configured() {
                                self.screen = Screen::Dashboard;
                                self.spawn_load_devices();
                            } else {
                                self.screen = Screen::Setup;
                            }
                        }
                    });
            }

            Screen::Setup => {
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(Colors::BG))
                    .show(ctx, |ui| {
                        let submit = crate::views::setup::render(
                            ui, &mut self.setup, &self.local_hostname
                        );
                        if submit && !self.setup.api_key.trim().is_empty() {
                            self.spawn_setup_connect();
                        }
                    });
            }

            Screen::Dashboard => {
                self.render_titlebar(ctx);
                self.render_statusbar(ctx);

                // ── Left: device panel ────────────────────────────────────────
                let mut device_clicked: Option<usize> = None;
                let mut needs_refresh = false;

                // Snapshot telemetry so the panel closure can read it without
                // holding a live borrow on self.telemetry_cache
                let telemetry_snap: HashMap<String, NodeTelemetry> =
                    self.telemetry_cache.clone();

                egui::SidePanel::left("devices_panel")
                    .exact_width(280.0)
                    .resizable(false)
                    .frame(egui::Frame::none()
                        .fill(Colors::BG_PANEL)
                        .stroke(egui::Stroke::new(1.0, Colors::BORDER))
                    )
                    .show(ctx, |ui| {
                        device_clicked = crate::views::dashboard::render_device_panel(
                            ui,
                            &self.devices,
                            &telemetry_snap,
                            self.selected_idx,
                            &mut self.device_filter,
                            &mut needs_refresh,
                        );
                    });

                if needs_refresh { self.spawn_load_devices(); }

                // Apply device selection reset
                if let Some(idx) = device_clicked {
                    if self.selected_idx != Some(idx) {
                        self.selected_idx = Some(idx);
                        self.active_tab   = DashTab::default();
                        self.remote_files.clear();
                        self.current_remote_path = PathBuf::new();
                        self.remote_model_edit = String::new();
                        self.is_tg_agent = false;
                    }
                }

                // ── Right: detail panel ───────────────────────────────────────
                egui::CentralPanel::default()
                    .frame(egui::Frame::none().fill(Colors::BG))
                    .show(ctx, |ui| {
                        if self.devices_loading && self.devices.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(200.0);
                                ui.spinner();
                                ui.add_space(12.0);
                                ui.label(RichText::new("SCANNING TAILSCALE NETWORK...")
                                    .color(Colors::TEXT_MUTED).size(10.0));
                            });
                            ctx.request_repaint();
                            return;
                        }

                        // Clone device to release the borrow on self.devices
                        let selected_device = self.selected_idx
                            .and_then(|i| self.devices.get(i))
                            .cloned();

                        if let Some(device) = selected_device {
                            // If Timeline tab just became active, load it
                            if self.active_tab == DashTab::Timeline
                                && self.timeline.needs_refresh()
                            {
                                let _ = self.event_tx.send(AppEvent::Status("Loading timeline...".into()));
                                self.spawn_load_timeline();
                            }

                            // Snapshot all read-only slices before any &mut borrows
                            let clip_snap    = self.clip_inbox.clone();
                            let queue_snap   = self.file_queue.clone();
                            let remote_snap  = self.remote_files.clone();
                            let log_snap     = self.transfer_log.clone();
                            let watch_snap   = self.watch_paths.clone();
                            let telem_snap   = telemetry_snap.get(&device.id).cloned();

                            let mut detail = DetailState {
                                device:         &device,
                                active_tab:     &mut self.active_tab,
                                rdp_username:   &mut self.rdp_username,
                                rdp_resolution: &mut self.rdp_resolution,
                                clip_out:       &mut self.clip_out,
                                clip_inbox:     &clip_snap,
                                file_queue:     &queue_snap,
                                remote_files:   &remote_snap,
                                transfer_log:   &log_snap,
                                is_tg_agent:    self.is_tg_agent,
                                watch_paths:    &watch_snap,
                                telemetry:      telem_snap.as_ref(),
                                current_remote_path: &mut self.current_remote_path,
                                remote_model_edit: &mut self.remote_model_edit,
                                terminal_view: self.terminal_sessions.get_mut(&device.id),
                            };

                            // Pass timeline state into the render
                            let actions = crate::views::dashboard::render_detail_panel_with_timeline(
                                ui, &mut detail, &mut self.timeline, &self.index_stats
                            );

                            // Auto-fetch telemetry when viewing a device
                            if let Some(ip) = device.primary_ip() {
                                self.spawn_get_telemetry(ip.to_string(), device.id.clone());
                            }

                            if let Some(ip) = device.primary_ip().map(|s| s.to_string()) {
                                self.handle_detail_actions(actions, &ip, &device.id);
                            }
                        } else {
                            crate::views::dashboard::render_empty_state(ui);
                        }
                    });

                // ── Overlays (rendered on top of everything) ──────────────────

                // Settings modal
                if render_settings_modal(ctx, &mut self.settings) {
                    let mut new_config = self.config.clone();
                    new_config.api_key      = self.settings.api_key.clone();
                    new_config.device_name  = self.settings.device_name.clone();
                    new_config.device_type  = self.settings.device_type.clone();
                    new_config.rdp_username = self.settings.rdp_username.clone();
                    new_config.agent_port   = self.settings.agent_port.parse().unwrap_or(47731);
                    new_config.ai_model     = if self.settings.ai_model.trim().is_empty() { None } else { Some(self.settings.ai_model.clone()) };
                    new_config.watch_paths  = self.settings.watch_paths.iter()
                        .map(|s| PathBuf::from(s))
                        .collect();

                    match new_config.save() {
                        Ok(_) => {
                            self.push_toast(Toast::ok("Settings saved"));
                            self.rdp_username = new_config.rdp_username.clone();

                            // Live update watcher
                            if let Some(fw) = &mut self.file_watcher {
                                // 1. Remove paths no longer in config
                                for old_path in &self.config.watch_paths {
                                    if !new_config.watch_paths.contains(old_path) {
                                        let _ = fw.unwatch(old_path);
                                    }
                                }
                                // 2. Add new paths
                                for new_path in &new_config.watch_paths {
                                    if !self.config.watch_paths.contains(new_path) {
                                        let _ = fw.watch(new_path.clone());
                                    }
                                }
                            }
                            
                            self.config      = new_config;
                            self.watch_paths = self.config.watch_paths.clone();
                            self.spawn_load_devices();
                        }
                        Err(e) => self.push_toast(Toast::err(format!("Save failed: {}", e))),
                    }
                }

                // Search overlay (Ctrl+F)
                let search_action = crate::views::search::render(
                    ctx, &mut self.search, &self.index_stats,
                    &mut self.semantic_enabled,
                    !self.semantic_loading,
                    self.embedding_progress
                );
                if search_action.closed {
                    self.search.results.clear();
                }
                if search_action.query_changed {
                    self.search_keystroke = Some(std::time::Instant::now());
                }
                if let Some(result) = search_action.open_result {
                    // Navigate to the owning device
                    if let Some(idx) = self.devices.iter().position(|d| d.id == result.device_id) {
                        self.selected_idx = Some(idx);
                        self.active_tab   = DashTab::Files;
                    }
                    self.push_toast(Toast::info(format!("Navigated to: {}", result.name)));
                }

                self.render_toasts(ctx);

                // --- Phase 3: Periodic Sync ---
                if self.mesh_sync_last_at.elapsed().as_secs() > 300 {
                    self.mesh_sync_last_at = std::time::Instant::now();
                    self.sync_all_nodes();
                }
            }
        }
    }
}
