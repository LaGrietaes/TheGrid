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
use std::sync::Arc;

use egui_tiles::Tree;
use egui::{Color32, Context, RichText};
use semver::Version;
use serde::Deserialize;

use thegrid_core::{AppEvent, Config};
use thegrid_core::models::*;
use thegrid_net::{TailscaleClient, RdpLauncher, AgentClient};
use thegrid_net::rdp::RdpResolution;
use thegrid_runtime::AppRuntime;

use crate::theme::Colors;
use crate::views::dashboard::{
    build_default_telemetry_tree, default_telemetry_band_height, DashTab, DetailState, DetailActions, SettingsState, TelemetryPane, render_settings_modal,
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

const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/LaGrietaes/TheGrid/releases/latest";

#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    html_url: String,
}

enum GitUpdateOutcome {
    UpToDate,
    Updated,
}

fn parse_version_tag(tag: &str) -> Option<Version> {
    let clean = tag.trim().trim_start_matches('v').trim_start_matches('V');
    Version::parse(clean).ok()
}

fn try_git_update() -> Result<GitUpdateOutcome, String> {
    let probe = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()
        .map_err(|e| e.to_string())?;

    if !probe.status.success() {
        return Err("Current directory is not a git repository".to_string());
    }

    let before = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .map_err(|e| e.to_string())?;
    if !before.status.success() {
        return Err("Failed to read current git HEAD".to_string());
    }
    let before_head = String::from_utf8_lossy(&before.stdout).trim().to_string();

    let fetch = std::process::Command::new("git")
        .arg("fetch")
        .arg("--prune")
        .output()
        .map_err(|e| e.to_string())?;
    if !fetch.status.success() {
        return Err(format!("git fetch failed: {}", String::from_utf8_lossy(&fetch.stderr).trim()));
    }

    let pull = std::process::Command::new("git")
        .arg("pull")
        .arg("--ff-only")
        .output()
        .map_err(|e| e.to_string())?;
    if !pull.status.success() {
        return Err(format!("git pull failed: {}", String::from_utf8_lossy(&pull.stderr).trim()));
    }

    let after = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .map_err(|e| e.to_string())?;
    if !after.status.success() {
        return Err("Failed to read updated git HEAD".to_string());
    }
    let after_head = String::from_utf8_lossy(&after.stdout).trim().to_string();

    if before_head == after_head {
        Ok(GitUpdateOutcome::UpToDate)
    } else {
        Ok(GitUpdateOutcome::Updated)
    }
}

fn try_rebuild_binaries() -> Result<(), String> {
    let build = std::process::Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("thegrid-node")
        .arg("-p")
        .arg("thegrid-gui")
        .output()
        .map_err(|e| e.to_string())?;

    if build.status.success() {
        Ok(())
    } else {
        Err(format!("cargo build failed: {}", String::from_utf8_lossy(&build.stderr).trim()))
    }
}

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
        Self { message: m.into(), color: Colors::GREEN,
               created: std::time::Instant::now(),
               duration: std::time::Duration::from_secs(3) }
    }
    fn is_expired(&self) -> bool { self.created.elapsed() > self.duration }
}

// ─────────────────────────────────────────────────────────────────────────────
// THE GRID App — owns ALL application state
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeStatus {
    Offline,      // Tailscale says it's off
    Reachable,    // Tailscale says it's on, but Agent not responding
    GridActive    // Agent is responding
}

#[derive(Debug, Clone, Default)]
pub struct GridScanProgress {
    pub machine_id: String,
    pub step: String,
    pub current_drive: String,
    pub current_sector: String,
    pub scanned: u64,
    pub total: u64,
    pub pending_sectors: u64,
    pub updated_at: i64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CloudPipelineProgress {
    pub stage: String,
    pub step: String,
    pub done: u64,
    pub total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub target: String,
    pub updated_at: i64,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NodeCrosscheckSummary {
    pub node_id: String,
    pub groups: u64,
    pub files: u64,
    pub bytes: u64,
    pub known_devices: u64,
    pub updated_at: i64,
}

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
    /// Phase 3: IDs of nodes currently participating in a cluster operation
    selected_node_ids:   Vec<String>,
    tailscale_connected: bool,
    active_tab:          DashTab,

    // ── Per-device RDP preferences ───────────────────────────────────────────
    rdp_usernames:   HashMap<String, String>,
    rdp_resolutions: HashMap<String, String>,
    is_tg_agent:    bool,

    // ── Clipboard / file transfer ─────────────────────────────────────────────
    clip_out:     String,
    clip_inbox:   Vec<ClipboardEntry>,
    file_queue:   Vec<FileQueueItem>,
    remote_files: Vec<RemoteFile>,
    transfer_log: Vec<TransferLogEntry>,

    /// The centralized engine for background tasks and services
    runtime: Arc<AppRuntime>,

    // ── Phase 2: File Manager ─────────────────────────────────────────────────
    file_manager: FileManagerState,
    duplicate_groups: Vec<(String, u64, Vec<FileSearchResult>)>,
    duplicate_last_scan: Option<i64>,
    grid_scan_progress: HashMap<String, GridScanProgress>,
    cloud_pipeline_progress: CloudPipelineProgress,
    node_crosscheck: HashMap<String, NodeCrosscheckSummary>,
    
    // ── Phase 3: SQLite index state (UI only) ─────────────────────────────────
    index_stats: IndexStats,

    // ── Phase 3: Search ───────────────────────────────────────────────────────
    search:           SearchPanelState,
    // Timestamp of last keypress — used for 300ms debounce
    search_keystroke: Option<std::time::Instant>,
    viewport:         ViewportState,

    timeline: TimelineState,
    /// Phase 3: Cluster View state (path per node)
    cluster_paths:    HashMap<String, PathBuf>,
    cluster_files:    HashMap<String, Vec<RemoteFile>>,

    // ── Phase 3: Project & Category State (Now in config) ─────────────────────

    mesh_sync_last_at: std::time::Instant,
    sync_last_poll: HashMap<String, std::time::Instant>,

    // --- Phase 4: Semantic AI UI State ---
    semantic_enabled:   bool,
    semantic_loading:   bool,
    embedding_progress: (usize, usize),
    hashing_progress:   (usize, usize),
    hashing_eta_secs:   Option<u64>,
    hashing_rate:       Option<f64>,
    hashing_last_tick:  Option<std::time::Instant>,

    // ── Phase 3: Telemetry cache ──────────────────────────────────────────────
    // key = Tailscale device_id, value = latest NodeTelemetry snapshot
    telemetry_cache: HashMap<String, NodeTelemetry>,
    // When we last polled each device for telemetry (to rate-limit polls)
    telemetry_last_poll: HashMap<String, std::time::Instant>,
    // When we last probed each remote device with /ping (separate from telemetry)
    ping_last_poll: HashMap<String, std::time::Instant>,
    telemetry_tree: Tree<TelemetryPane>,
    telemetry_band_height: f32,
    // Whether a local telemetry collection is in flight
    local_telemetry_pending: bool,
    // When the in-flight local telemetry collection started (watchdog)
    local_telemetry_pending_since: Option<std::time::Instant>,

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

    /// New in Node Enhancement: tracks the provider URL being typed in the UI
    remote_url_edit: String,

    // --- Phase 4: Idle & Persistence ---
    last_input_at: std::time::Instant,
    idle_notified: bool,
    initial_scan_dispatched: bool,
    release_check_dispatched: bool,

    // ── Phase 4: Device collaboration state ───────────────────────────────────
    /// Derived GUI state per device_id — drives status dot color and label.
    device_display_states: HashMap<String, DeviceDisplayState>,
    /// In-flight compute borrow sessions — drives ComputeBorrowing state.
    compute_sessions: Vec<ComputeSession>,

    // ── Phase 6: Dedup review ─────────────────────────────────────────────────
    rich_duplicate_groups: Vec<DuplicateGroup>,
    dedup_review_state: crate::views::dedup_review::DedupReviewState,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FileViewMode {
    #[default]
    List,
    Grid,
}

#[allow(dead_code)]
pub struct FileManagerState {
    pub current_path:    std::path::PathBuf,
    pub selected_files:  std::collections::HashSet<String>,
    pub view_mode:       FileViewMode,
    pub _show_hidden:    bool,
    pub _last_refresh:   Option<std::time::Instant>,
    pub filter_query:    String,
    pub sort_by_name:    bool,
    pub sort_ascending:  bool,
    /// Preview: the name of the file currently being previewed
    pub preview_file:    Option<String>,
    /// Preview: raw bytes content (comes from the agent)
    pub preview_content: Option<Vec<u8>>,
    /// Preview: OS-provided texture for image files
    pub preview_texture: Option<egui::TextureHandle>,
    /// Active SmartRule ID for filtering the current view
    pub active_rule:     Option<String>,
    pub duplicate_min_size_mb: u64,
    pub duplicate_ext_filter: String,
    pub duplicate_path_filter: String,
    pub duplicate_max_groups: usize,
    pub duplicate_expanded_groups: std::collections::HashSet<String>,
    pub duplicate_selected_files: std::collections::HashSet<i64>,
    pub drive_remote: String,
    pub drive_last_manifest: Option<std::path::PathBuf>,
}
impl Default for FileManagerState {
    fn default() -> Self {
        Self {
            current_path:    std::path::PathBuf::new(),
            selected_files:  std::collections::HashSet::new(),
            view_mode:       FileViewMode::List,
            _show_hidden:    false,
            _last_refresh:   None,
            filter_query:    String::new(),
            sort_by_name:    true,
            sort_ascending:  true,
            preview_file:    None,
            preview_content: None,
            preview_texture: None,
            active_rule:     None,
            duplicate_min_size_mb: 0,
            duplicate_ext_filter: String::new(),
            duplicate_path_filter: String::new(),
            duplicate_max_groups: 200,
            duplicate_expanded_groups: std::collections::HashSet::new(),
            duplicate_selected_files: std::collections::HashSet::new(),
            drive_remote: "gdrive:THEGRID-BUFFER".to_string(),
            drive_last_manifest: None,
        }
    }
}

#[derive(Default)]
pub struct ViewportState {
    pub active_file: Option<FileSearchResult>,
    pub content:     String,
    pub is_loading:  bool,
    pub preview_kind: PreviewKind,
}

impl TheGridApp {
    fn usb_discovery_enabled() -> bool {
        std::env::var("THEGRID_ENABLE_USB_DISCOVERY")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    fn is_local_device(&self, device: &TailscaleDevice) -> bool {
        let host = self.local_hostname.as_str();
        let configured = self.config.device_name.as_str();
        device.hostname.eq_ignore_ascii_case(host)
            || device.name.eq_ignore_ascii_case(host)
            || device.display_name().eq_ignore_ascii_case(host)
            || device.hostname.eq_ignore_ascii_case(configured)
            || device.name.eq_ignore_ascii_case(configured)
            || device.display_name().eq_ignore_ascii_case(configured)
    }

    fn list_usb_adb_devices(&self) -> Vec<TailscaleDevice> {
        if !Self::command_exists("adb") {
            return Vec::new();
        }

        let output = match std::process::Command::new("adb")
            .arg("devices")
            .arg("-l")
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        for line in stdout.lines().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if !line.contains("\tdevice") || !line.contains("usb:") {
                continue;
            }

            let mut parts = line.split_whitespace();
            let serial = match parts.next() {
                Some(s) => s.to_string(),
                None => continue,
            };

            let mut model = None;
            for token in line.split_whitespace() {
                if let Some(m) = token.strip_prefix("model:") {
                    model = Some(m.to_string());
                    break;
                }
                if model.is_none() {
                    if let Some(d) = token.strip_prefix("device:") {
                        model = Some(d.to_string());
                    }
                }
            }

            let label = model
                .unwrap_or_else(|| "ANDROID_USB".to_string())
                .replace('_', "-")
                .to_uppercase();

            devices.push(TailscaleDevice {
                id: format!("adb-usb-{}", serial),
                hostname: format!("{}-USB", label),
                name: format!("{} (USB ADB)", label),
                addresses: Vec::new(),
                os: "Android".to_string(),
                client_version: "USB ADB".to_string(),
                last_seen: Some(chrono::Utc::now()),
                blocks_incoming: false,
                authorized: true,
                user: "USB".to_string(),
            });
        }

        devices
    }

    fn command_exists(cmd: &str) -> bool {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("where")
                .arg(cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("which")
                .arg(cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    }

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

        // Initialize the shared runtime
        let runtime = Arc::new(AppRuntime::new(config.clone(), tx.clone())
            .expect("Failed to initialize AppRuntime"));
        runtime.start_services();

        Self {
            screen:      Screen::Boot,
            boot_start:  std::time::Instant::now(),
            config: config.clone(),
            setup,
            settings,
            devices: Vec::new(),
            devices_loading: false,
            device_filter: String::new(),
            selected_idx: None,
            selected_node_ids: Vec::new(),
            tailscale_connected: false,
            active_tab:     DashTab::default(),
            rdp_usernames:   HashMap::new(),
            rdp_resolutions: HashMap::new(),
            is_tg_agent: false,
            clip_out:   String::new(),
            clip_inbox: Vec::new(),
            file_queue:   Vec::new(),
            remote_files: Vec::new(),
            transfer_log: Vec::new(),
            
            runtime,
            file_manager: {
                let mut fm = FileManagerState::default();
                if let Some(remote) = config.drive_buffer_remote.clone() {
                    fm.drive_remote = remote;
                }
                fm
            },
            duplicate_groups: Vec::new(),
            duplicate_last_scan: None,
            grid_scan_progress: HashMap::new(),
            cloud_pipeline_progress: CloudPipelineProgress::default(),
            node_crosscheck: HashMap::new(),
            index_stats:  IndexStats::default(),
            search:           SearchPanelState::default(),
            search_keystroke: None,
            timeline: TimelineState::default(),
            viewport: ViewportState::default(),
            telemetry_cache:     HashMap::new(),
            telemetry_last_poll: HashMap::new(),
            ping_last_poll:      HashMap::new(),
            telemetry_tree: build_default_telemetry_tree(),
            telemetry_band_height: default_telemetry_band_height(),
            local_telemetry_pending: false,
            local_telemetry_pending_since: None,
            event_tx: tx,
            event_rx: rx,
            toasts: Vec::new(),
            status_msg: "READY".into(),
            mesh_sync_last_at: std::time::Instant::now(),
            sync_last_poll: HashMap::new(),
            

            
            // --- Phase 4: UI state (kept in app) ---
            semantic_enabled:  false,
            semantic_loading:  true,
            embedding_progress: (0, 0),
            hashing_progress:   (0, 0),
            hashing_eta_secs:   None,
            hashing_rate:       None,
            hashing_last_tick:  None,
            cluster_paths: HashMap::new(),
            cluster_files: HashMap::new(),
            local_hostname,
            current_remote_path: PathBuf::new(),
            remote_model_edit: String::new(),
            remote_url_edit: String::new(),
            terminal_sessions: HashMap::new(),
            last_input_at: std::time::Instant::now(),
            idle_notified: false,
            initial_scan_dispatched: false,
            release_check_dispatched: false,
            device_display_states: HashMap::new(),
            compute_sessions: Vec::new(),
            rich_duplicate_groups: Vec::new(),
            dedup_review_state: crate::views::dedup_review::DedupReviewState::default(),
        }
    }

    fn start_release_check(&mut self) {
        if self.release_check_dispatched {
            return;
        }
        self.release_check_dispatched = true;

        let skip_update_check = std::env::var("THEGRID_SKIP_UPDATE_CHECK")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if skip_update_check {
            return;
        }

        let auto_update = std::env::var("THEGRID_AUTO_UPDATE")
            .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
            .unwrap_or(true);

        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let client = match reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
            {
                Ok(c) => c,
                Err(_) => return,
            };

            let response = match client
                .get(RELEASES_LATEST_URL)
                .header("User-Agent", format!("thegrid-gui/{}", env!("CARGO_PKG_VERSION")))
                .send()
            {
                Ok(r) => r,
                Err(_) => return,
            };

            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return;
            }

            let release = match response.error_for_status().and_then(|r| r.json::<ReleaseInfo>()) {
                Ok(r) => r,
                Err(_) => return,
            };

            let current = match parse_version_tag(env!("CARGO_PKG_VERSION")) {
                Some(v) => v,
                None => return,
            };
            let latest = match parse_version_tag(&release.tag_name) {
                Some(v) => v,
                None => return,
            };

            if latest > current {
                if auto_update {
                    let _ = tx.send(AppEvent::Status(format!(
                        "update_available:{}|{}",
                        release.tag_name,
                        release.html_url
                    )));
                    match try_git_update() {
                        Ok(GitUpdateOutcome::UpToDate) => {
                            let _ = tx.send(AppEvent::Status("update_latest".to_string()));
                        }
                        Ok(GitUpdateOutcome::Updated) => {
                            match try_rebuild_binaries() {
                                Ok(()) => {
                                    let _ = tx.send(AppEvent::Status("update_applied_restart_gui".to_string()));
                                }
                                Err(e) => {
                                    let _ = tx.send(AppEvent::Status(format!("update_failed:{}", e)));
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(AppEvent::Status(format!("update_failed:{}", e)));
                        }
                    }
                } else {
                    let _ = tx.send(AppEvent::Status(format!(
                        "update_available:{}|{}",
                        release.tag_name,
                        release.html_url
                    )));
                }
            }
        });
    }

    fn start_initial_watch_scans(&mut self) {
        if self.initial_scan_dispatched {
            return;
        }
        self.initial_scan_dispatched = true;

        let cfg = self.runtime.config.lock().unwrap().clone();
        if cfg.watch_paths.is_empty() {
            self.set_status("No watch paths configured. Add one to start indexing.");
            self.push_toast(Toast::info("No watch paths configured yet."));
            return;
        }

        self.push_toast(Toast::info(format!(
            "Starting initial indexing for {} watch path(s)",
            cfg.watch_paths.len()
        )));
        let resuming = self.runtime.db.lock()
            .ok()
            .and_then(|db| db.has_pending_index_tasks().ok())
            .unwrap_or(false);
        if resuming {
            self.set_status("Resuming unfinished indexing tasks...");
        } else {
            self.set_status(format!("Starting indexing for {} watch path(s)...", cfg.watch_paths.len()));
        }

        self.runtime.spawn_index_directories(
            cfg.watch_paths,
            cfg.device_name.clone(),
            cfg.device_name.clone(),
        );
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Background task spawners
    // ─────────────────────────────────────────────────────────────────────────

    fn spawn_load_devices(&mut self) {
        if self.devices_loading { return; }
        self.devices_loading = true;
        self.set_status("Fetching devices from Tailscale...");
        self.runtime.spawn_load_devices();
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

    fn spawn_ping(&mut self, ip: String, device_id: String, manual: bool) {
        let now = std::time::Instant::now();
        self.ping_last_poll.insert(device_id, now);
        self.runtime.spawn_ping(ip, manual);
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
        self.runtime.spawn_list_remote_files(ip);
    }

    fn spawn_send_file(&self, device_id: String, path: PathBuf, queue_idx: usize) {
        let best_ip = self.find_best_ip(&device_id);
        log::info!("BPW: Sending file to {} via {} (queue_idx={})", device_id, best_ip, queue_idx);
        self.runtime.spawn_send_file(best_ip, path, queue_idx);
    }

    fn spawn_download_file(&self, ip: String, filename: String) {
        self.runtime.spawn_download_file(ip, filename);
    }

    fn spawn_browse_remote_directory(&self, ip: String, device_id: String, path: PathBuf) {
        self.runtime.spawn_browse_remote_directory(ip, device_id, path);
    }

    /// Browse the LOCAL filesystem directly — no agent call needed.
    fn spawn_local_browse(&self, device_id: String, path: PathBuf) {
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let browse_path = if path.as_os_str().is_empty() {
                // No path — list drives on Windows, or / on other OS
                #[cfg(target_os = "windows")]
                {
                    // List logical drives
                    let drives: Vec<RemoteFile> = ('A'..='Z')
                        .filter_map(|c| {
                            let p = PathBuf::from(format!("{}:\\", c));
                            if p.exists() {
                                Some(RemoteFile {
                                    name:   format!("{}:\\", c),
                                    size:   0,
                                    is_dir: true,
                                    modified: None,
                                })
                            } else {
                                None
                            }
                        })
                        .collect();
                    let _ = tx.send(AppEvent::RemoteBrowseLoaded { device_id, path, files: drives });
                    return;
                }
                #[cfg(not(target_os = "windows"))]
                PathBuf::from("/")
            } else {
                path.clone()
            };

            match std::fs::read_dir(&browse_path) {
                Ok(entries) => {
                    let mut files: Vec<RemoteFile> = entries
                        .filter_map(|e| e.ok())
                        .filter_map(|entry| {
                            let meta = entry.metadata().ok()?;
                            let name = entry.file_name().to_string_lossy().to_string();
                            // Skip hidden files (starting with '.')
                            if name.starts_with('.') { return None; }
                            Some(RemoteFile {
                                name:     name,
                                size:     if meta.is_file() { meta.len() } else { 0 },
                                is_dir:   meta.is_dir(),
                                modified: None,
                            })
                        })
                        .collect();
                    // Sort: dirs first, then by name
                    files.sort_by(|a, b| {
                        if a.is_dir != b.is_dir { b.is_dir.cmp(&a.is_dir) }
                        else { a.name.to_lowercase().cmp(&b.name.to_lowercase()) }
                    });
                    let _ = tx.send(AppEvent::RemoteBrowseLoaded { device_id, path: browse_path, files });
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::RemoteBrowseFailed { device_id, error: e.to_string() });
                }
            }
        });
    }

    /// Cluster View: browse a specific node's directory and store result in cluster_files
    fn spawn_load_cluster_path(&self, device_id: String, path: PathBuf) {
        // Find the IP for this device
        if let Some(ip) = self.devices.iter()
            .find(|d| d.id == device_id)
            .and_then(|d| d.primary_ip())
            .map(|s| s.to_string())
        {
            self.runtime.spawn_browse_remote_directory(ip, device_id, path);
        }
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

    fn spawn_update_remote_config(&self, ip: String, device_id: String, device_type: Option<String>, model: Option<String>, url: Option<String>) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.update_config(device_type, model, url)) {
                Ok(_) => { let _ = tx.send(AppEvent::RemoteConfigUpdated { device_id }); }
                Err(e) => { let _ = tx.send(AppEvent::RemoteConfigFailed { device_id, error: e.to_string() }); }
            }
        });
    }

    #[allow(dead_code)]
    fn spawn_fm_delete(&self, ip: String, _device_id: String, paths: Vec<String>) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key) {
                Ok(client) => {
                    let mut count = 0;
                    for path in paths {
                        if let Err(e) = client.delete_file(&path) {
                            let _ = tx.send(AppEvent::Status(format!("Delete failed: {}", e)));
                        } else {
                            count += 1;
                        }
                    }
                    let _ = tx.send(AppEvent::Status(format!("{} items deleted", count)));
                    let _ = tx.send(AppEvent::RequestRefresh);
                }
                Err(e) => { let _ = tx.send(AppEvent::Status(format!("Agent connection failed: {}", e))); }
            }
        });
    }

    #[allow(dead_code)]
    fn spawn_fm_rename(&self, ip: String, _device_id: String, old_path: String, new_name: String) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.rename_file(&old_path, &new_name)) {
                Ok(_) => {
                    let _ = tx.send(AppEvent::Status("Item renamed".into()));
                    let _ = tx.send(AppEvent::RequestRefresh);
                }
                Err(e) => { let _ = tx.send(AppEvent::Status(format!("Rename failed: {}", e))); }
            }
        });
    }

    #[allow(dead_code)]
    fn spawn_fm_move(&self, ip: String, _device_id: String, paths: Vec<String>, dest_dir: PathBuf) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        let dest_str = dest_dir.to_string_lossy().to_string();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.move_files(paths, &dest_str)) {
                Ok(_) => {
                    let _ = tx.send(AppEvent::Status("Items moved".into()));
                    let _ = tx.send(AppEvent::RequestRefresh);
                }
                Err(e) => { let _ = tx.send(AppEvent::Status(format!("Move failed: {}", e))); }
            }
        });
    }

    fn spawn_enable_rdp(&self, ip: String, device_id: String) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.enable_rdp()) {
                Ok(_)  => { let _ = tx.send(AppEvent::RdpEnabled { device_id }); }
                Err(e) => { let _ = tx.send(AppEvent::RdpFailed { device_id, error: e.to_string() }); }
            }
        });
    }

    // ── Phase 3: Index spawners ───────────────────────────────────────────────

    /// Kick off a full directory walk for a newly added watch path.
    fn spawn_index_directory(&mut self, path: PathBuf, device_id: String, device_name: String) {
        self.index_stats.reset_scan();
        self.grid_scan_progress.insert(device_id.clone(), GridScanProgress {
            machine_id: device_name.clone(),
            step: "indexing".to_string(),
            current_drive: path.to_string_lossy().to_string(),
            current_sector: path.to_string_lossy().to_string(),
            scanned: 0,
            total: 0,
            pending_sectors: 0,
            updated_at: chrono::Utc::now().timestamp(),
            last_error: None,
        });
        self.runtime.spawn_index_directory(path, device_id, device_name);
    }

    /// Incrementally re-index a set of changed paths (from FileSystemChanged).
    fn spawn_incremental_index(&self, changes: Vec<thegrid_core::FileChange>) {
        self.runtime.spawn_incremental_index(changes);
    }

    /// Run an FTS5 search. Generation counter prevents stale results overwriting
    /// newer ones if multiple searches are in flight simultaneously.
    /// Sync a single remote node's index delta. (Phase 3)
    fn spawn_sync_node(&self, device_id: String, ip: String, hostname: String) {
        self.runtime.spawn_sync_node(device_id, ip, hostname);
    }

    fn spawn_sync_node_if_due(&mut self, device_id: String, ip: String, hostname: String, min_interval_secs: u64) {
        let now = std::time::Instant::now();
        if min_interval_secs > 0 {
            if let Some(last) = self.sync_last_poll.get(&device_id) {
                if last.elapsed().as_secs() < min_interval_secs {
                    return;
                }
            }
        }
        self.sync_last_poll.insert(device_id.clone(), now);
        self.spawn_sync_node(device_id, ip, hostname);
    }

    /// Pull index deltas from ALL reachable Tailscale nodes. (Phase 3)
    fn sync_all_nodes(&mut self) {
        log::debug!("Initiating mesh index synchronization...");
        let local_name = self.config.device_name.clone();
        let devices_snapshot = self.devices.clone();
        let mut targets = Vec::new();
        for device in &devices_snapshot {
            if device.name == local_name { continue; }
            if let Some(ip) = device.primary_ip() {
                targets.push((device.id.clone(), ip.to_string(), device.hostname.clone()));
            }
        }
        for (device_id, ip, hostname) in targets {
            self.grid_scan_progress.insert(device_id.clone(), GridScanProgress {
                machine_id: hostname.clone(),
                step: "syncing".to_string(),
                current_drive: "remote".to_string(),
                current_sector: "pulling index delta".to_string(),
                scanned: 0,
                total: 0,
                pending_sectors: 0,
                updated_at: chrono::Utc::now().timestamp(),
                last_error: None,
            });
            self.spawn_sync_node_if_due(device_id, ip, hostname, 45);
        }
    }


    /// Background worker that processes files and generates embeddings.
    fn spawn_embedding_worker(&self) {
        self.runtime.spawn_embedding_worker();
    }

    fn spawn_hashing_worker(&self) {
        self.runtime.spawn_hashing_worker();
    }

    fn spawn_search(&mut self) {
        let query     = self.search.query.trim().to_string();
        if query.is_empty() {
            self.search.results.clear();
            self.search.searching = false;
            return;
        }

        let _gen     = self.search.mark_dispatched();
        let device_filter = self.search.device_filter.clone();
        let semantic_enabled = self.semantic_enabled;

        self.runtime.spawn_search(query, device_filter, semantic_enabled);
    }

    /// Fetch hardware telemetry from a remote agent (rate-limited to once per 15s).
    fn spawn_get_telemetry(&mut self, ip: String, device_id: String) {
        let now = std::time::Instant::now();
        if let Some(&last) = self.telemetry_last_poll.get(&device_id) {
            if last.elapsed().as_secs() < 15 { return; }
        }
        self.telemetry_last_poll.insert(device_id.clone(), now);

        self.runtime.spawn_get_telemetry(ip, device_id);
    }

    /// Collect telemetry for the LOCAL machine via sysinfo (non-blocking).
    fn spawn_collect_local_telemetry(&mut self, device_id: String) {
        if self.local_telemetry_pending { return; }
        self.local_telemetry_pending = true;
        self.local_telemetry_pending_since = Some(std::time::Instant::now());

        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let telemetry = crate::telemetry::collect_local();
            let _ = tx.send(AppEvent::TelemetryUpdate { device_id, ip: None, telemetry });
        });
    }

    /// Send a Wake-on-LAN magic packet.
    /// `mac_addr` format: "AA:BB:CC:DD:EE:FF"

    /// Load recent files from SQLite for the Timeline view.
    fn spawn_load_timeline(&mut self) {
        if self.timeline.loading { return; }
        self.timeline.loading = true;
        let device_filter = self.timeline.device_filter.clone();
        self.runtime.spawn_load_timeline(device_filter);
    }

    fn enqueue_local_full_drive_index(&mut self) {
        let local_id = self.config.device_name.clone();
        let drives = self
            .telemetry_cache
            .get(&local_id)
            .map(|t| t.capabilities.drives.clone())
            .unwrap_or_default();
        for drive in drives {
            let root = PathBuf::from(&drive.name);
            if !thegrid_core::should_skip_path(&root) {
                self.grid_scan_progress.insert(local_id.clone(), GridScanProgress {
                    machine_id: local_id.clone(),
                    step: "queueing drives".to_string(),
                    current_drive: root.to_string_lossy().to_string(),
                    current_sector: "root".to_string(),
                    scanned: 0,
                    total: 0,
                    pending_sectors: 0,
                    updated_at: chrono::Utc::now().timestamp(),
                    last_error: None,
                });
                self.runtime
                    .spawn_index_directory(root, local_id.clone(), local_id.clone());
            }
        }
    }

    fn spawn_duplicate_scan(&self, filter: DuplicateScanFilter) {
        self.runtime.spawn_duplicates_scan_filtered(filter);
    }

    fn spawn_rich_dedup_delete(&mut self, files: Vec<FileSearchResult>) {
        let db        = Arc::clone(&self.runtime.db);
        let tx        = self.event_tx.clone();
        let local_dev = self.config.device_name.clone();
        let session   = uuid::Uuid::new_v4().to_string();

        std::thread::spawn(move || {
            let mut deleted = 0usize;
            let mut errors  = 0usize;

            for f in &files {
                let is_local = f.device_id == local_dev;

                if is_local {
                    if let Err(e) = std::fs::remove_file(&f.path) {
                        log::warn!("[DedupDelete] remove_file {:?}: {}", f.path, e);
                        errors += 1;
                        continue;
                    }
                }

                if let Ok(guard) = db.lock() {
                    let _ = guard.log_deletion(
                        &session,
                        &f.path.to_string_lossy(),
                        &f.device_id,
                        f.hash.as_deref(),
                        Some(f.size),
                        if is_local { "local_remove" } else { "remote_remove_requested" },
                        Some("dedup_review"),
                    );
                    let _ = guard.delete_file_by_id(f.id);
                }
                deleted += 1;
            }

            let msg = if errors > 0 {
                format!("Dedup: deleted {}, {} error(s)", deleted, errors)
            } else {
                format!("Dedup: deleted {} duplicate(s)", deleted)
            };
            let _ = tx.send(AppEvent::Status(msg));
        });
    }

    fn spawn_delete_duplicate_files(&mut self, files: Vec<(i64, std::path::PathBuf, String)>, filter: DuplicateScanFilter) {
        let db = Arc::clone(&self.runtime.db);
        let runtime = Arc::clone(&self.runtime);
        let tx = self.event_tx.clone();
        let local_device = self.config.device_name.clone();

        for (id, _, _) in &files {
            self.file_manager.duplicate_selected_files.remove(id);
        }

        std::thread::spawn(move || {
            let mut deleted = 0usize;
            let mut errors = 0usize;

            for (id, path, device_id) in &files {
                if device_id == &local_device {
                    if let Err(e) = std::fs::remove_file(path) {
                        log::warn!("[Duplicates] Failed to remove {}: {}", path.display(), e);
                        errors += 1;
                        continue;
                    }
                }
                if let Ok(guard) = db.lock() {
                    if let Err(e) = guard.delete_file_by_id(*id) {
                        log::warn!("[Duplicates] DB delete id={}: {}", id, e);
                    }
                }
                deleted += 1;
            }

            let _ = tx.send(AppEvent::Status(
                if errors > 0 {
                    format!("Deleted {} duplicate(s), {} error(s)", deleted, errors)
                } else {
                    format!("Deleted {} duplicate(s)", deleted)
                }
            ));

            runtime.spawn_duplicates_scan_filtered(filter);
        });
    }

    fn classify_for_buffer(name: &str) -> String {
        let ext = std::path::Path::new(name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "tif" | "tiff" | "svg" => "images".to_string(),
            "mp4" | "mkv" | "mov" | "avi" | "webm" | "flv" | "mpeg" | "mpg" => "video".to_string(),
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => "audio".to_string(),
            "pdf" | "doc" | "docx" | "txt" | "md" | "odt" => "documents".to_string(),
            "zip" | "rar" | "7z" | "tar" | "gz" => "archives".to_string(),
            _ => "other".to_string(),
        }
    }

    fn spawn_export_drive_buffer(&self) {
        let cfg = self.runtime.config.lock().ok().map(|c| c.clone()).unwrap_or_default();
        let groups = self.duplicate_groups.clone();
        let tx = self.event_tx.clone();

        std::thread::spawn(move || {
            let _ = tx.send(AppEvent::Status(format!(
                "cloud_progress:export|prepare|0|{}|0|0|local-buffer",
                groups.len()
            )));
            if !cfg.drive_buffer_enabled {
                let _ = tx.send(AppEvent::Status("drive_buffer_error:Drive buffer is disabled in config".to_string()));
                return;
            }

            if groups.is_empty() {
                let _ = tx.send(AppEvent::Status("drive_buffer_error:No duplicate groups available. Run duplicate analysis first".to_string()));
                return;
            }

            let now = chrono::Utc::now();
            let session_id = now.format("%Y%m%d_%H%M%S").to_string();
            let session_root = cfg.effective_drive_buffer_dir().join(&session_id);
            let staged_root = session_root.join("staged");
            if let Err(e) = std::fs::create_dir_all(&staged_root) {
                let _ = tx.send(AppEvent::Status(format!("drive_buffer_error:Failed to create staging folder: {}", e)));
                return;
            }

            let mut entries: Vec<DriveBufferEntry> = Vec::new();
            let mut staged_total_bytes = 0u64;
            let mut source_files = 0usize;
            let mut exported_groups = 0u64;

            for (hash, size, files) in &groups {
                source_files += files.len();
                let primary = files.iter().max_by_key(|f| (f.modified.unwrap_or(0), f.indexed_at));
                let Some(primary) = primary else { continue; };
                if !primary.path.exists() || !primary.path.is_file() {
                    continue;
                }

                let category = Self::classify_for_buffer(&primary.name);
                let hash_short_len = std::cmp::min(10, hash.len());
                let hash_short = &hash[..hash_short_len];
                let fallback_name = format!("{}_file", hash_short);
                let base_name = if primary.name.trim().is_empty() { fallback_name } else { primary.name.clone() };
                let mut staged_rel = std::path::PathBuf::from(&category)
                    .join(&primary.device_id)
                    .join(format!("{}_{}", hash_short, base_name));
                let mut staged_abs = staged_root.join(&staged_rel);

                if staged_abs.exists() {
                    staged_rel = std::path::PathBuf::from(&category)
                        .join(&primary.device_id)
                        .join(format!("{}_{}_{}", hash_short, primary.indexed_at, base_name));
                    staged_abs = staged_root.join(&staged_rel);
                }

                if let Some(parent) = staged_abs.parent() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        let _ = tx.send(AppEvent::Status(format!(
                            "drive_buffer_warn:Skip {} (mkdir failed: {})",
                            primary.path.display(),
                            e
                        )));
                        continue;
                    }
                }

                if let Err(e) = std::fs::copy(&primary.path, &staged_abs) {
                    let _ = tx.send(AppEvent::Status(format!(
                        "drive_buffer_warn:Skip {} (copy failed: {})",
                        primary.path.display(),
                        e
                    )));
                    continue;
                }

                let sidecar_name = staged_abs
                    .file_name()
                    .map(|n| format!("{}.thegrid.json", n.to_string_lossy()))
                    .unwrap_or_else(|| "metadata.thegrid.json".to_string());
                let sidecar_path = staged_abs.with_file_name(sidecar_name);
                let sidecar = serde_json::json!({
                    "source_path": primary.path,
                    "device_id": primary.device_id,
                    "device_name": primary.device_name,
                    "hash": hash,
                    "size": size,
                    "duplicate_group_size": files.len(),
                    "redundant_bytes": size.saturating_mul(files.len().saturating_sub(1) as u64),
                    "category": category,
                    "indexed_at": primary.indexed_at,
                    "generated_at": now.timestamp(),
                });
                let _ = std::fs::write(&sidecar_path, serde_json::to_vec_pretty(&sidecar).unwrap_or_default());

                entries.push(DriveBufferEntry {
                    source_path: primary.path.clone(),
                    staged_path: staged_rel,
                    device_id: primary.device_id.clone(),
                    category,
                    hash: hash.clone(),
                    size: *size,
                    duplicate_group_size: files.len(),
                    indexed_at: primary.indexed_at,
                });
                staged_total_bytes = staged_total_bytes.saturating_add(*size);
                exported_groups += 1;
                let _ = tx.send(AppEvent::Status(format!(
                    "cloud_progress:export|copying|{}|{}|{}|0|{}",
                    exported_groups,
                    groups.len(),
                    staged_total_bytes,
                    session_root.display()
                )));
            }

            let manifest = DriveBufferManifest {
                generated_at: now.timestamp(),
                session_id: session_id.clone(),
                quota_tb: cfg.drive_buffer_quota_tb,
                source_groups: groups.len(),
                source_files,
                staged_files: entries.len(),
                staged_total_bytes,
                root_folder: session_root.clone(),
                entries,
            };

            let manifest_path = session_root.join("manifest.json");
            match serde_json::to_vec_pretty(&manifest)
                .ok()
                .and_then(|bytes| std::fs::write(&manifest_path, bytes).ok())
            {
                Some(_) => {
                    let _ = tx.send(AppEvent::Status(format!(
                        "cloud_progress:export|complete|{}|{}|{}|{}|{}",
                        manifest.staged_files,
                        manifest.source_groups,
                        manifest.staged_total_bytes,
                        manifest.staged_total_bytes,
                        session_root.display()
                    )));
                    let _ = tx.send(AppEvent::Status(format!(
                        "drive_buffer_manifest:{}|{}|{}",
                        manifest_path.display(),
                        manifest.staged_files,
                        manifest.staged_total_bytes
                    )));
                }
                None => {
                    let _ = tx.send(AppEvent::Status("drive_buffer_error:Failed to write buffer manifest".to_string()));
                }
            }
        });
    }

    fn spawn_upload_drive_buffer(&self, remote_override: Option<String>) {
        let manifest_path = self.file_manager.drive_last_manifest.clone();
        let cfg = self.runtime.config.lock().ok().map(|c| c.clone()).unwrap_or_default();
        let tx = self.event_tx.clone();

        std::thread::spawn(move || {
            let Some(manifest_path) = manifest_path else {
                let _ = tx.send(AppEvent::Status("drive_upload_err:No manifest to upload. Export buffer first".to_string()));
                return;
            };

            let session_root = manifest_path.parent().map(|p| p.to_path_buf());
            let Some(session_root) = session_root else {
                let _ = tx.send(AppEvent::Status("drive_upload_err:Invalid manifest location".to_string()));
                return;
            };

            let remote = remote_override
                .filter(|r| !r.trim().is_empty())
                .or_else(|| cfg.drive_buffer_remote.clone())
                .unwrap_or_else(|| "gdrive:THEGRID-BUFFER".to_string());

            let _ = tx.send(AppEvent::Status(format!(
                "cloud_progress:upload|prepare|0|0|0|0|{}",
                remote
            )));

            let probe = if cfg!(target_os = "windows") {
                std::process::Command::new("where").arg("rclone").output()
            } else {
                std::process::Command::new("which").arg("rclone").output()
            };
            match probe {
                Ok(out) if out.status.success() => {}
                _ => {
                    let _ = tx.send(AppEvent::Status(format!(
                        "cloud_progress:upload|error|0|0|0|0|{}",
                        remote
                    )));
                    let _ = tx.send(AppEvent::Status(
                        "drive_upload_err:rclone not found. Install rclone and configure a Google Drive remote".to_string()
                    ));
                    return;
                }
            }

            let output = std::process::Command::new("rclone")
                .arg("copy")
                .arg(&session_root)
                .arg(&remote)
                .arg("--create-empty-src-dirs")
                .arg("--transfers")
                .arg("8")
                .output();

            let _ = tx.send(AppEvent::Status(format!(
                "cloud_progress:upload|running|0|0|0|0|{}",
                remote
            )));

            match output {
                Ok(out) if out.status.success() => {
                    let _ = tx.send(AppEvent::Status(format!(
                        "cloud_progress:upload|complete|0|0|0|0|{}",
                        remote
                    )));
                    let _ = tx.send(AppEvent::Status(format!(
                        "drive_upload_ok:{}|{}",
                        session_root.display(),
                        remote
                    )));
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    let _ = tx.send(AppEvent::Status(format!(
                        "cloud_progress:upload|error|0|0|0|0|{}",
                        remote
                    )));
                    let _ = tx.send(AppEvent::Status(format!(
                        "drive_upload_err:{}",
                        if err.is_empty() { "Upload failed" } else { &err }
                    )));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Status(format!(
                        "cloud_progress:upload|error|0|0|0|0|{}",
                        remote
                    )));
                    let _ = tx.send(AppEvent::Status(format!("drive_upload_err:{}", e)));
                }
            }
        });
    }

    /// Update index_stats from the DB (cheap count query, safe to call often).
    fn refresh_index_stats(&mut self) {
        let db = Arc::clone(&self.runtime.db);
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
            if thegrid_core::should_skip_path(&path) {
                self.push_toast(Toast::err("That folder is considered system/software and is blocked for indexing"));
                self.set_status(format!("Blocked watch path: {}", path.display()));
                return;
            }

            let is_watching = self.runtime.config.lock().unwrap().watch_paths.contains(&path);
            if is_watching {
                self.push_toast(Toast::info("Already watching that directory"));
                return;
            }
            
            let rt = Arc::clone(&self.runtime);
            let mut watcher_lock = rt.file_watcher.lock().unwrap();
            match &mut *watcher_lock {
                None => {
                    drop(watcher_lock);
                    self.push_toast(Toast::err("File watcher unavailable"));
                }
                Some(fw) => {
                    match fw.watch(path.clone()) {
                        Ok(_) => {
                            let label = path.file_name()
                                .unwrap_or_default().to_string_lossy().to_string();
                            {
                                let mut cfg = rt.config.lock().unwrap();
                                cfg.watch_paths.push(path.clone());
                                if let Err(e) = cfg.save() {
                                    self.push_toast(Toast::err(format!("Saved watcher but failed to persist config: {}", e)));
                                }
                            }

                            // Keep local app config/settings mirror in sync so the UI reflects
                            // the new path immediately without reopening the app.
                            self.config = rt.config.lock().unwrap().clone();
                            self.settings.watch_paths = self
                                .config
                                .watch_paths
                                .iter()
                                .map(|p| p.to_string_lossy().to_string())
                                .collect();

                            drop(watcher_lock);
                            self.push_toast(Toast::ok(format!("Watching: {}", label)));
                            self.set_status(format!("Watching + indexing: {}", path.display()));

                            // Kick off a full index scan for the new path
                            let dev_id   = self.config.device_name.clone();
                            let dev_name = self.config.device_name.clone();
                            self.spawn_index_directory(path, dev_id, dev_name);
                        }
                        Err(e) => {
                            drop(watcher_lock);
                            self.push_toast(Toast::err(format!("Watch failed: {}", e)));
                        }
                    }
                }
            }
        }
    }

    // Helper: set_status from a non-&mut self context (used in spawner closures)

    // ─────────────────────────────────────────────────────────────────────────
    // Event processor — drains mpsc channel every frame
    // ─────────────────────────────────────────────────────────────────────────

    fn process_events(&mut self) {
        // Keep frames responsive even when watcher/index workers emit bursts of events.
        const MAX_EVENTS_PER_FRAME: usize = 300;
        let mut processed = 0usize;

        while processed < MAX_EVENTS_PER_FRAME {
            let event = match self.event_rx.try_recv() {
                Ok(event) => event,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            };
            processed += 1;
            match event {

                // ── Devices ───────────────────────────────────────────────────
                AppEvent::DevicesLoaded(mut devices) => {
                    self.devices_loading     = false;
                    self.tailscale_connected = true;
                    let n = devices.len();
                    let previous_selected_id = self
                        .selected_idx
                        .and_then(|i| self.devices.get(i))
                        .map(|d| d.id.clone());

                    // Register all devices in DB without blocking the UI thread.
                    if let Ok(guard) = self.runtime.db.try_lock() {
                        for d in &devices {
                            let _ = guard.upsert_device(&d.id, d.display_name());
                        }
                    }

                    // Prioritize the local node by moving it to the top of the list.
                    if let Some(local_idx) = devices.iter().position(|d| self.is_local_device(d)) {
                        let local_node = devices.remove(local_idx);
                        devices.insert(0, local_node);
                    }

                    // USB discovery can block on some ADB installs, so keep it opt-in.
                    if Self::usb_discovery_enabled() {
                        let adb_usb_devices = self.list_usb_adb_devices();
                        if !adb_usb_devices.is_empty() {
                            for adb_dev in adb_usb_devices {
                                let already_exists = devices.iter().any(|d| d.id == adb_dev.id);
                                if !already_exists {
                                    devices.push(adb_dev);
                                }
                            }
                        }
                    }

                    self.devices = devices;
                    if let Some(selected_id) = previous_selected_id {
                        self.selected_idx = self.devices.iter().position(|d| d.id == selected_id);
                    }
                    if self.selected_idx.is_none() {
                        self.selected_idx = self.devices.iter().position(|d| self.is_local_device(d));
                    }
                    if self.selected_idx.is_none() && !self.devices.is_empty() {
                        self.selected_idx = Some(0);
                    }
                    self.set_status(format!("{} nodes discovered", n));

                    // Share peer list with runtime so the compute router can use them
                    self.runtime.update_tailscale_peers(self.devices.clone());

                    // Start local telemetry collection immediately after first load
                    if let Some(local_device_id) = self
                        .devices
                        .iter()
                        .find(|d| self.is_local_device(d))
                        .map(|d| d.id.clone())
                    {
                        self.spawn_collect_local_telemetry(local_device_id);
                    }
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
                AppEvent::AgentPingOk { ip, response, manual } => {
                    self.is_tg_agent = true;
                    if response.authorized {
                        if manual {
                            self.push_toast(Toast::ok(format!("⬡ Agent online: {} (v{})", response.device, response.version)));
                        }
                        // Find the device ID for this IP to trigger telemetry
                        let device_id = self.devices.iter()
                            .find(|d| d.primary_ip() == Some(&ip))
                            .map(|d| d.id.clone())
                            .unwrap_or_else(|| response.device.clone());

                        self.mark_device_online(&device_id.clone());
                        self.spawn_get_telemetry(ip, device_id);
                    } else {
                        if manual {
                            self.push_toast(Toast::info(format!("⬡ Agent online: {} (v{}) - Limited Access (Key Mismatch)", response.device, response.version)));
                        }
                        self.set_status("Authentication mismatch: please check your api_key");
                    }
                }
                AppEvent::AgentPingFailed { ip, error, manual } => {
                    self.is_tg_agent = false;
                    if manual {
                        self.push_toast(Toast::err(format!("Agent ping failed: {}", error)));
                    }
                    self.set_status(format!("Ping failed. Check port {} and firewall.", self.config.agent_port));
                    // Mark the device offline in display state
                    if let Some(device_id) = self.devices.iter()
                        .find(|d| d.primary_ip() == Some(&ip))
                        .map(|d| d.id.clone())
                    {
                        self.mark_device_offline(&device_id);
                    }
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

                AppEvent::AgentFilePreviewLoaded(content) => {
                    self.file_manager.preview_content = Some(content);
                }

                AppEvent::RemoteBrowseLoaded { device_id, path, files } => {
                    // Cluster view: update per-node file state if applicable
                    if self.selected_node_ids.contains(&device_id) {
                        self.cluster_files.insert(device_id.clone(), files.clone());
                        self.cluster_paths.insert(device_id.clone(), path.clone());
                    }
                    // Single-node view: also update the legacy remote_files state
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
                AppEvent::FileSystemChanged { changes, summary } => {
                    self.set_status(format!("⬡ {}", summary));
                    // Phase 3: trigger incremental index update
                    self.spawn_incremental_index(changes);
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
                AppEvent::SyncRequest { after, requester_device, response_tx } => {
                    let db = self.runtime.db.clone();
                    std::thread::spawn(move || {
                        if let Ok(guard) = db.lock() {
                            let delta = guard
                                .get_sync_delta_after_filtered(after, requester_device.as_deref())
                                .unwrap_or_default();
                            let _ = response_tx.send(delta);
                        }
                    });
                }

                AppEvent::SyncComplete { device_id, files_added } => {
                    log::info!("Sync complete for {}: {} items", device_id, files_added);
                    let now = chrono::Utc::now().timestamp();
                    let entry = self.grid_scan_progress.entry(device_id.clone()).or_default();
                    if entry.machine_id.is_empty() {
                        entry.machine_id = device_id.clone();
                    }
                    entry.step = "sync complete".to_string();
                    entry.current_drive = "remote".to_string();
                    entry.current_sector = "delta applied".to_string();
                    entry.scanned = files_added as u64;
                    entry.total = files_added as u64;
                    entry.pending_sectors = 0;
                    entry.updated_at = now;
                    entry.last_error = None;
                    self.refresh_index_stats();
                    self.index_stats.scanning = false;
                }
                AppEvent::SyncFailed { device_id, error } => {
                    log::debug!("Sync failed for {}: {}", device_id, error);
                    let now = chrono::Utc::now().timestamp();
                    let short_error = error
                        .split(':')
                        .last()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| error.clone());
                    let entry = self.grid_scan_progress.entry(device_id.clone()).or_default();
                    if entry.machine_id.is_empty() {
                        entry.machine_id = device_id;
                    }
                    entry.step = "sync failed".to_string();
                    entry.current_drive = "remote".to_string();
                    entry.current_sector = "sync request failed".to_string();
                    entry.updated_at = now;
                    entry.last_error = Some(short_error);
                    self.index_stats.scanning = false;
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
                AppEvent::HashingProgress { hashed, total } => {
                    let now = std::time::Instant::now();
                    if let Some(last_tick) = self.hashing_last_tick {
                        let dt = last_tick.elapsed().as_secs_f64().max(0.001);
                        let d_hashed = hashed.saturating_sub(self.hashing_progress.0) as f64;
                        if d_hashed > 0.0 {
                            let instant_rate = d_hashed / dt;
                            self.hashing_rate = Some(match self.hashing_rate {
                                Some(prev) => prev * 0.75 + instant_rate * 0.25,
                                None => instant_rate,
                            });
                        }
                    }
                    self.hashing_last_tick = Some(now);
                    self.hashing_progress = (hashed, total);
                    if let Some(rate) = self.hashing_rate {
                        if rate > 0.0 && total > hashed {
                            self.hashing_eta_secs = Some(((total - hashed) as f64 / rate) as u64);
                        } else {
                            self.hashing_eta_secs = None;
                        }
                    }
                }

                AppEvent::SemanticFailed(err) => {
                    log::error!("Semantic AI failure: {}", err);
                    self.semantic_loading = false;
                    self.push_toast(Toast::err(format!("AI failed: {}", err)));
                }

                // ── Phase 3: Index ────────────────────────────────────────────
                AppEvent::IndexProgress { scanned, total, current, ext, estimated_total } => {
                    if !self.index_stats.scanning {
                        self.index_stats.reset_scan();
                    }
                    self.index_stats.scan_progress = scanned;
                    self.index_stats.scan_total    = total;

                    if let Some(e) = ext {
                        let count = self.index_stats.type_counts.entry(e).or_insert(0);
                        *count += 1;
                    }

                    // Stable ETA using smoothed files/s from progress deltas.
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    if let Some(last_ts) = self.index_stats.last_progress_ts {
                        let dt = (now - last_ts).max(1) as f64;
                        let dfiles = scanned.saturating_sub(self.index_stats.last_progress_scanned) as f64;
                        if dfiles > 0.0 {
                            let instant_rate = dfiles / dt;
                            let smooth = match self.index_stats.smoothed_files_per_sec {
                                Some(prev) => prev * 0.75 + instant_rate * 0.25,
                                None => instant_rate,
                            };
                            self.index_stats.smoothed_files_per_sec = Some(smooth);
                        }
                    }
                    self.index_stats.last_progress_ts = Some(now);
                    self.index_stats.last_progress_scanned = scanned;

                    if let Some(rate) = self.index_stats.smoothed_files_per_sec {
                        if rate > 0.0 {
                            let remaining = total.saturating_sub(scanned);
                            self.index_stats.scan_eta_secs = Some((remaining as f64 / rate) as u64);
                        }
                    }

                    if total > 0 {
                        if estimated_total {
                            self.set_status(format!("Indexing~: {}/{} ({})", scanned, total, current));
                        } else {
                            self.set_status(format!("Indexing: {}/{} ({})", scanned, total, current));
                        }
                    } else {
                        self.set_status(format!("Indexing (Resuming...): {} ({})", scanned, current));
                    }
                }

                AppEvent::IndexComplete { device_id, files_added, duration_ms } => {
                    self.index_stats.scanning    = false;
                    self.index_stats.total_files += files_added;
                    self.index_stats.last_scanned = Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64
                    );
                    self.index_stats.scan_eta_secs = None;
                    self.index_stats.smoothed_files_per_sec = None;
                    self.set_status(format!(
                        "Index complete: {} files in {:.1}s",
                        files_added, duration_ms as f64 / 1000.0
                    ));
                    self.push_toast(Toast::ok(format!(
                        "Indexed {} files", files_added
                    )));
                    let now = chrono::Utc::now().timestamp();
                    let entry = self.grid_scan_progress.entry(device_id.clone()).or_default();
                    if entry.machine_id.is_empty() {
                        entry.machine_id = device_id;
                    }
                    entry.step = "index complete".to_string();
                    entry.current_sector = "completed".to_string();
                    entry.scanned = files_added;
                    entry.total = files_added;
                    entry.pending_sectors = 0;
                    entry.updated_at = now;
                    entry.last_error = None;
                    self.refresh_index_stats();

                    // Phase 4: Trigger embedding generation for the new files
                    if !self.semantic_loading && self.runtime.is_ai_node {
                        self.spawn_embedding_worker();
                    }

                    // Phase 2: Trigger background hashing
                    self.spawn_hashing_worker();
                }

                AppEvent::RequestIdleWork => {
                    self.runtime.spawn_idle_work();
                }
                
                AppEvent::UserIdle(_) => {
                    // Could pause indexing here if needed
                }
                AppEvent::IndexUpdated { paths_updated } => {
                    if paths_updated > 0 {
                        self.set_status(format!("⬡ Incremental index: {} items updated", paths_updated));
                        self.refresh_index_stats();

                        // Phase 4: Trigger embedding generation for the changes
                        if !self.semantic_loading && self.runtime.is_ai_node {
                            self.spawn_embedding_worker();
                        }

                        // Phase 2: Trigger background hashing
                        self.spawn_hashing_worker();
                    }
                }

                // ── Phase 3: Search ───────────────────────────────────────────
                AppEvent::SearchResults(results) => {
                    // Generation-tagged results arrive via Status("search_gen:N")
                    // before SearchResults — handled below. Accept all results for now.
                    self.search.receive_results(self.search.query_gen, results);
                }

                AppEvent::DuplicatesFound(groups) => {
                    self.duplicate_groups = groups;
                    self.duplicate_last_scan = Some(chrono::Utc::now().timestamp());

                    if self.duplicate_groups.is_empty() {
                        self.push_toast(Toast::info("Duplicate scan completed: no groups found"));
                    } else {
                        let wasted: u64 = self.duplicate_groups
                            .iter()
                            .map(|(_, size, files)| size.saturating_mul(files.len().saturating_sub(1) as u64))
                            .sum();
                        self.push_toast(Toast::ok(format!(
                            "Duplicate scan: {} groups, {:.2} GB recoverable",
                            self.duplicate_groups.len(),
                            wasted as f64 / 1_073_741_824.0
                        )));
                    }
                }

                // ── Phase 4: Remote AI Request Handling (for node side) ────────
                AppEvent::RemoteAiEmbedRequest { text, response_tx } => {
                    self.runtime.handle_remote_ai_embed(text, response_tx);
                }
                AppEvent::RemoteAiSearchRequest { query, k, response_tx } => {
                    self.runtime.handle_remote_ai_search(query, k, response_tx);
                }

                // ── Phase 3: Telemetry ────────────────────────────────────────
                AppEvent::TelemetryUpdate { device_id, ip, telemetry } => {
                    let remote_device_id = device_id.clone();
                    let telemetry_ip = ip.clone();
                    // Mark local telemetry as no longer pending
                    if ip.is_none() {
                        self.local_telemetry_pending = false;
                        self.local_telemetry_pending_since = None;
                    }

                    // Synchronize the runtime's remote AI map
                    if let Some(ip_addr) = ip {
                        let mut nodes = self.runtime.remote_ai_nodes.lock().unwrap();
                        if telemetry.is_ai_capable {
                            nodes.insert(device_id.clone(), ip_addr);
                        } else {
                            nodes.remove(&device_id);
                        }
                    }

                    self.telemetry_cache.insert(device_id.clone(), telemetry);

                    // Keep remote indexes synced as telemetry confirms a responsive node.
                    if let Some(remote_device) = self.devices.iter().find(|d| d.id == remote_device_id) {
                        if remote_device.name != self.config.device_name {
                            let best_ip = telemetry_ip
                                .or_else(|| remote_device.primary_ip().map(|s| s.to_string()))
                                .unwrap_or_else(|| self.find_best_ip(&remote_device_id));
                            self.spawn_sync_node_if_due(
                                remote_device_id,
                                best_ip,
                                remote_device.hostname.clone(),
                                30,
                            );
                        }
                    }
                }

                // ── Phase 3: WoL ──────────────────────────────────────────────
                AppEvent::WolSent { device_name, target_mac: _ } => {
                    self.push_toast(Toast::ok(format!("⚡ Wake packet sent to {}", device_name)));
                }
                AppEvent::WolFailed { reason } => {
                    self.push_toast(Toast::err(format!("WoL failed: {}", reason)));
                }

                // ── RDP Support ───────────────────────────────────────────────
                AppEvent::EnableRdp { ip, device_id } => {
                    self.spawn_enable_rdp(ip, device_id);
                    self.set_status("Enabling RDP on remote node...");
                }
                AppEvent::RdpEnabled { device_id } => {
                    self.push_toast(Toast::ok("RDP enabled successfully"));
                    self.set_status("RDP is now active");
                    // Force a telemetry refresh to update the UI button
                    if let Some(d) = self.devices.iter().find(|d| d.id == device_id) {
                        if let Some(ip) = d.primary_ip() {
                            self.spawn_get_telemetry(ip.to_string(), device_id);
                        }
                    }
                }
                AppEvent::RdpFailed { device_id: _, error } => {
                    self.push_toast(Toast::err(format!("RDP Enablement failed: {}", error)));
                    self.set_status(format!("Error: {}", error));
                }

                // ── Phase 3: Timeline ─────────────────────────────────────────
                AppEvent::TemporalLoaded(entries) => {
                    self.timeline.entries = entries;
                    self.timeline.mark_refreshed();
                }

                AppEvent::RequestRefresh => {
                    if let Some(device) = self.selected_idx.and_then(|i| self.devices.get(i)) {
                        if let Some(ip) = device.primary_ip().map(|s| s.to_string()) {
                            if self.current_remote_path.as_os_str().is_empty() {
                                self.spawn_list_remote_files(ip);
                            } else {
                                self.spawn_browse_remote_directory(ip, device.id.clone(), self.current_remote_path.clone());
                            }
                        }
                    }
                }

                // ── UI / misc ─────────────────────────────────────────────────
                AppEvent::Status(msg) => {
                    // Special-cased status messages used as piggyback channels
                    if msg.starts_with("index_count:") {
                        if let Ok(n) = msg["index_count:".len()..].parse::<u64>() {
                            self.index_stats.total_files = n;
                        }
                    } else if msg.starts_with("update_available:") {
                        let payload = &msg["update_available:".len()..];
                        let mut parts = payload.splitn(2, '|');
                        let version = parts.next().unwrap_or("unknown");
                        let url = parts.next().unwrap_or("");
                        self.push_toast(Toast::info(format!("Update available: {}", version)));
                        if !url.is_empty() {
                            self.set_status(format!("New version {} available: {}", version, url));
                        } else {
                            self.set_status(format!("New version {} available", version));
                        }
                    } else if msg == "update_applied_restart_gui" {
                        self.push_toast(Toast::ok("Update applied. Restart THE GRID to run the latest version."));
                        self.set_status("Updated binaries successfully. Restart required.");
                    } else if msg == "update_latest" {
                        self.set_status("Already on latest version.");
                    } else if msg.starts_with("update_failed:") {
                        let detail = &msg["update_failed:".len()..];
                        self.push_toast(Toast::err(format!("Auto-update failed: {}", detail)));
                        self.set_status("Auto-update failed. Check logs/status and retry manually.");
                    } else if msg.starts_with("config_update:") {
                        // Format: config_update:model="...",url="..."
                        // Simple parser for node-side config syncing
                        let parts = &msg["config_update:".len()..];
                        let mut model = None;
                        let mut url = None;
                        
                        for part in parts.split(',') {
                            if part.starts_with("model=") {
                                let val = part["model=".len()..].trim_matches('"').to_string();
                                if val != "None" && !val.is_empty() { model = Some(val); }
                            } else if part.starts_with("url=") {
                                let val = part["url=".len()..].trim_matches('"').to_string();
                                if val != "None" && !val.is_empty() { url = Some(val); }
                            }
                        }

                        let mut cfg = self.config.clone();
                        let mut changed = false;
                        if model.is_some() { cfg.ai_model = model; changed = true; }
                        if url.is_some() { cfg.ai_provider_url = url; changed = true; }
                        
                        if changed {
                            log::info!("[Node] Updating local config from remote command: {:?}", cfg);
                            let _ = cfg.save();
                            {
                                let mut runtime_cfg = self.runtime.config.lock().unwrap();
                                *runtime_cfg = cfg.clone();
                            }
                            self.config = cfg;
                            self.runtime.refresh_ai_services();
                        }

                    } else if msg.starts_with("grid_scan_drive_start:") {
                        let payload = &msg["grid_scan_drive_start:".len()..];
                        let mut parts = payload.splitn(2, '|');
                        if let (Some(machine), Some(drive)) = (parts.next(), parts.next()) {
                            let now = chrono::Utc::now().timestamp();
                            let entry = self.grid_scan_progress.entry(machine.to_string()).or_default();
                            if entry.machine_id.is_empty() {
                                entry.machine_id = machine.to_string();
                            }
                            entry.step = "indexing".to_string();
                            entry.current_drive = drive.to_string();
                            entry.current_sector = "root".to_string();
                            entry.updated_at = now;
                            entry.last_error = None;
                        }

                    } else if msg.starts_with("grid_sync_start:") {
                        let payload = &msg["grid_sync_start:".len()..];
                        let parts: Vec<&str> = payload.split('|').collect();
                        if parts.len() >= 2 {
                            let machine = parts[0].to_string();
                            let host = parts[1].to_string();
                            let now = chrono::Utc::now().timestamp();
                            let entry = self.grid_scan_progress.entry(machine.clone()).or_default();
                            if entry.machine_id.is_empty() {
                                entry.machine_id = host;
                            }
                            entry.step = "syncing".to_string();
                            entry.current_drive = "remote".to_string();
                            entry.current_sector = "requesting index sync".to_string();
                            entry.updated_at = now;
                            entry.last_error = None;
                        }

                    } else if msg.starts_with("grid_scan_progress:") {
                        let payload = &msg["grid_scan_progress:".len()..];
                        let parts: Vec<&str> = payload.split('|').collect();
                        if parts.len() >= 6 {
                            let machine = parts[0].to_string();
                            let drive = parts[1].to_string();
                            let sector = parts[2].to_string();
                            let scanned = parts[3].parse::<u64>().unwrap_or(0);
                            let total = parts[4].parse::<u64>().unwrap_or(0);
                            let pending = parts[5].parse::<u64>().unwrap_or(0);
                            let now = chrono::Utc::now().timestamp();

                            let entry = self.grid_scan_progress.entry(machine.clone()).or_default();
                            if entry.machine_id.is_empty() {
                                entry.machine_id = machine;
                            }
                            entry.step = "indexing".to_string();
                            entry.current_drive = drive;
                            entry.current_sector = sector;
                            entry.scanned = scanned;
                            entry.total = total;
                            entry.pending_sectors = pending;
                            entry.updated_at = now;
                            entry.last_error = None;
                        }

                    } else if msg.starts_with("grid_scan_complete:") {
                        let payload = &msg["grid_scan_complete:".len()..];
                        let parts: Vec<&str> = payload.split('|').collect();
                        if parts.len() >= 3 {
                            let machine = parts[0].to_string();
                            let scanned = parts[1].parse::<u64>().unwrap_or(0);
                            let now = chrono::Utc::now().timestamp();
                            let entry = self.grid_scan_progress.entry(machine.clone()).or_default();
                            if entry.machine_id.is_empty() {
                                entry.machine_id = machine;
                            }
                            entry.step = "index complete".to_string();
                            entry.current_sector = "completed".to_string();
                            entry.scanned = scanned;
                            entry.total = scanned;
                            entry.pending_sectors = 0;
                            entry.updated_at = now;
                            entry.last_error = None;
                        }

                    } else if msg.starts_with("cloud_progress:") {
                        let payload = &msg["cloud_progress:".len()..];
                        let parts: Vec<&str> = payload.split('|').collect();
                        if parts.len() >= 7 {
                            self.cloud_pipeline_progress.stage = parts[0].to_string();
                            self.cloud_pipeline_progress.step = parts[1].to_string();
                            self.cloud_pipeline_progress.done = parts[2].parse::<u64>().unwrap_or(0);
                            self.cloud_pipeline_progress.total = parts[3].parse::<u64>().unwrap_or(0);
                            self.cloud_pipeline_progress.bytes_done = parts[4].parse::<u64>().unwrap_or(0);
                            self.cloud_pipeline_progress.bytes_total = parts[5].parse::<u64>().unwrap_or(0);
                            self.cloud_pipeline_progress.target = parts[6].to_string();
                            self.cloud_pipeline_progress.updated_at = chrono::Utc::now().timestamp();
                            if self.cloud_pipeline_progress.step.eq_ignore_ascii_case("error") {
                                self.cloud_pipeline_progress.last_error = Some("cloud stage failed".to_string());
                            } else {
                                self.cloud_pipeline_progress.last_error = None;
                            }
                        }

                    } else if msg.starts_with("crosscheck:") {
                        let payload = &msg["crosscheck:".len()..];
                        let parts: Vec<&str> = payload.split('|').collect();
                        if parts.len() >= 5 {
                            let node_id = parts[0].to_string();
                            let groups = parts[1].parse::<u64>().unwrap_or(0);
                            let files = parts[2].parse::<u64>().unwrap_or(0);
                            let bytes = parts[3].parse::<u64>().unwrap_or(0);
                            let known_devices = parts[4].parse::<u64>().unwrap_or(0);
                            self.node_crosscheck.insert(node_id.clone(), NodeCrosscheckSummary {
                                node_id,
                                groups,
                                files,
                                bytes,
                                known_devices,
                                updated_at: chrono::Utc::now().timestamp(),
                            });
                        }

                    } else if msg.starts_with("drive_buffer_manifest:") {
                        let payload = &msg["drive_buffer_manifest:".len()..];
                        let parts: Vec<&str> = payload.split('|').collect();
                        if let Some(path) = parts.first() {
                            self.file_manager.drive_last_manifest = Some(PathBuf::from(path));
                        }
                        self.push_toast(Toast::ok("Drive buffer exported with manifest"));
                    } else if msg.starts_with("drive_buffer_error:") {
                        self.push_toast(Toast::err(msg["drive_buffer_error:".len()..].to_string()));
                    } else if msg.starts_with("drive_buffer_warn:") {
                        self.push_toast(Toast::info(msg["drive_buffer_warn:".len()..].to_string()));
                    } else if msg.starts_with("drive_upload_ok:") {
                        let payload = &msg["drive_upload_ok:".len()..];
                        self.push_toast(Toast::ok(format!("Drive upload complete: {}", payload)));
                    } else if msg.starts_with("drive_upload_err:") {
                        self.push_toast(Toast::err(msg["drive_upload_err:".len()..].to_string()));

                    } else if !msg.starts_with("search_gen:") {
                        // Regular status messages go to the status bar
                        if msg.to_lowercase().contains("failed") || msg.to_lowercase().contains("error") {
                            self.push_toast(Toast::err(msg.clone()));
                        }
                        self.set_status(msg);
                    }
                }
                AppEvent::EnableAdb { ip, api_key } => {
                    if !Self::command_exists("adb") {
                        self.push_toast(Toast::err("ADB not found. Install Android Platform Tools and add adb to PATH."));
                        self.set_status("ADB missing on this machine");
                        continue;
                    }
                    if !Self::command_exists("scrcpy") {
                        self.push_toast(Toast::err("scrcpy not found. Install scrcpy and add it to PATH."));
                        self.set_status("scrcpy missing on this machine");
                        continue;
                    }

                    let tx = self.event_tx.clone();
                    let port = self.config.agent_port;
                    std::thread::spawn(move || {
                        let client = reqwest::blocking::Client::new();
                        let url = format!("http://{}:{}/adb/enable", ip, port);
                        log::info!("Preparing remote node {} for mirroring...", ip);
                        let _ = tx.send(AppEvent::Status(format!("Enabling ADB on {}...", ip)));
                        
                        match client.post(&url)
                            .header("X-Grid-Key", &api_key)
                            .timeout(std::time::Duration::from_secs(5))
                            .send() 
                        {
                            Ok(resp) => {
                                if resp.status().is_success() {
                                    log::info!("ADB enabled on remote. Connecting...");
                                    let _ = tx.send(AppEvent::Status("ADB enabled. Connecting...".to_string()));
                                    
                                    // Clear stale connections
                                    let _ = std::process::Command::new("adb").arg("disconnect").output();
                                    std::thread::sleep(std::time::Duration::from_millis(1000));
                                    
                                    let addr = format!("{}:5555", ip);

                                    // Connect with retries. adb can report either
                                    // "connected to" or "already connected to".
                                    let mut connected = false;
                                    let mut last_error = String::new();
                                    for _attempt in 0..3 {
                                        let output = std::process::Command::new("adb")
                                            .arg("connect")
                                            .arg(&addr)
                                            .output();

                                        match output {
                                            Ok(out) => {
                                                let stdout = String::from_utf8_lossy(&out.stdout);
                                                let stderr = String::from_utf8_lossy(&out.stderr);
                                                let combined = format!("{}{}", stdout, stderr).to_lowercase();
                                                if combined.contains("connected to") || combined.contains("already connected to") {
                                                    connected = true;
                                                    break;
                                                }
                                                last_error = combined.trim().to_string();
                                            }
                                            Err(e) => {
                                                last_error = format!("Could not execute adb: {}", e);
                                            }
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(700));
                                    }

                                    if connected {
                                        let _ = tx.send(AppEvent::Status(format!("Connected to {}", ip)));
                                    } else {
                                        log::warn!("ADB connect fallback for {}: {}", ip, last_error);
                                        let _ = tx.send(AppEvent::Status(format!("ADB connect retry fallback for {}", ip)));
                                    }

                                    // scrcpy --tcpip can establish/repair the session itself.
                                    log::info!("Launching scrcpy...");
                                    let _ = std::process::Command::new("scrcpy")
                                        .arg(format!("--tcpip={}", ip))
                                        .spawn();
                                } else {
                                    let _ = tx.send(AppEvent::Status(format!("Enable failed: {}", resp.status())));
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(AppEvent::Status(format!("Node unreachable: {}", e)));
                            }
                        }
                    });
                }
                AppEvent::OpenSettings   => { self.settings.open = true; }

                // ── Preview ───────────────────────────────────────────────────
                AppEvent::RequestFilePreview(file) => {
                    self.viewport.active_file = Some(file.clone());
                    self.viewport.is_loading = true;
                    self.viewport.content.clear();
                    self.spawn_fetch_preview(file);
                }
                AppEvent::FilePreviewLoaded { file_id: _, content, kind } => {
                    self.viewport.is_loading = false;
                    self.viewport.content = content;
                    self.viewport.preview_kind = kind;
                }

                // ── Compute sharing ───────────────────────────────────────────
                AppEvent::ComputeBorrowOk { task_id, provider_device_id, task_type } => {
                    self.compute_sessions.push(ComputeSession {
                        borrower_device_id: self.config.device_name.clone(),
                        provider_device_id: provider_device_id.clone(),
                        task_type,
                        task_id,
                        started_at: std::time::Instant::now(),
                    });
                    self.refresh_device_display_states();
                    self.push_toast(Toast::info(format!("Compute delegated to {}", provider_device_id)));
                }

                AppEvent::ComputeBorrowFailed { task_id: _, reason } => {
                    self.push_toast(Toast::err(format!("Compute delegation failed: {}", reason)));
                }

                AppEvent::ComputeTaskUpdate(progress) => {
                    use thegrid_core::models::ComputeTaskState;
                    if matches!(progress.state, ComputeTaskState::Done | ComputeTaskState::Failed | ComputeTaskState::Cancelled) {
                        self.compute_sessions.retain(|s| s.task_id != progress.task_id);
                        self.refresh_device_display_states();
                    }
                }

                // ── Google Drive ──────────────────────────────────────────────
                AppEvent::DriveAuthExpired => {
                    self.push_toast(Toast::err("Google Drive token expired — re-authenticate in Settings"));
                }
                AppEvent::DriveIndexProgress { indexed, total } => {
                    let total_str = total.map(|t| t.to_string()).unwrap_or_else(|| "?".to_string());
                    self.set_status(format!("Drive: indexing {}/{}", indexed, total_str));
                }
                AppEvent::DriveIndexComplete { indexed } => {
                    self.push_toast(Toast::ok(format!("Drive: indexed {} files", indexed)));
                }
                AppEvent::DriveIndexError(msg) => {
                    self.push_toast(Toast::err(format!("Drive error: {}", msg)));
                }

                // ── Rich duplicate groups ─────────────────────────────────────
                AppEvent::DuplicatesGrouped(groups) => {
                    log::info!("DuplicatesGrouped: {} groups", groups.len());
                    self.dedup_review_state.seed_from_groups(&groups);
                    self.rich_duplicate_groups = groups;
                    self.push_toast(Toast::info(format!("{} duplicate groups found", self.rich_duplicate_groups.len())));
                }

                _ => {}
            }
        }
    }

    // ── Device display state derivation ───────────────────────────────────────

    /// Rebuild `device_display_states` for all known devices from current app state.
    fn refresh_device_display_states(&mut self) {
        let devices: Vec<_> = self.devices.iter()
            .map(|d| (d.id.clone(), d.hostname.clone()))
            .collect();

        for (device_id, hostname) in &devices {
            let state = self.derive_display_state(device_id, hostname);
            self.device_display_states.insert(device_id.clone(), state);
        }
    }

    fn derive_display_state(&self, device_id: &str, _hostname: &str) -> DeviceDisplayState {
        // Error: agent ping explicitly failed
        // (We don't track per-device ping failures currently, so skip for now.)

        // ComputeBorrowing: we have an active session delegated TO this device
        if self.compute_sessions.iter().any(|s| s.provider_device_id == device_id) {
            return DeviceDisplayState::ComputeBorrowing;
        }

        // ComputeProviding: a remote session is running ON THIS machine
        // (would come from a separate field; placeholder)

        // Indexing: the device is in an active index scan
        if self.index_stats.scanning && device_id == self.config.device_name {
            return DeviceDisplayState::Indexing;
        }

        // Syncing: a sync poll is in progress for this device
        if self.sync_last_poll.contains_key(device_id) {
            let elapsed = self.sync_last_poll[device_id].elapsed().as_secs();
            if elapsed < 5 {
                return DeviceDisplayState::Syncing;
            }
        }

        // Online / Offline: based on last ping outcome
        // (stored in device_display_states itself as a base, or derived from NodeStatus)
        // Default: check existing state or fall back to Offline.
        self.device_display_states
            .get(device_id)
            .cloned()
            .unwrap_or(DeviceDisplayState::Offline)
    }

    /// Called when a ping succeeds — marks device Online (or keeps higher-precedence state).
    fn mark_device_online(&mut self, device_id: &str) {
        let current = self.device_display_states.get(device_id).cloned().unwrap_or(DeviceDisplayState::Offline);
        if current.precedence() < DeviceDisplayState::Online.precedence() {
            self.device_display_states.insert(device_id.to_string(), DeviceDisplayState::Online);
        }
    }

    /// Called when a ping fails — marks device Offline only if no higher state is set.
    fn mark_device_offline(&mut self, device_id: &str) {
        let current = self.device_display_states.get(device_id).cloned().unwrap_or(DeviceDisplayState::Offline);
        if current == DeviceDisplayState::Online {
            self.device_display_states.insert(device_id.to_string(), DeviceDisplayState::Offline);
        }
    }

    fn spawn_fetch_preview(&self, file: thegrid_core::models::FileSearchResult) {
        use thegrid_core::models::PreviewKind;
        let tx = self.event_tx.clone();
        
        std::thread::spawn(move || {
            // Determine PreviewKind based on extension
            let ext = file.ext.clone().unwrap_or_default().to_lowercase();
            let kind = match ext.as_str() {
                "txt" | "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "md" | "json" | "toml" | "yaml" | "yml" | "iss" | "ps1" => PreviewKind::Text,
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => PreviewKind::Image,
                "pdf" => PreviewKind::Pdf,
                _ => PreviewKind::UnSupported
            };

            if kind == PreviewKind::Text {
                match std::fs::read_to_string(&file.path) {
                    Ok(content) => {
                        let _ = tx.send(AppEvent::FilePreviewLoaded { file_id: file.id, content, kind });
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::FilePreviewLoaded { file_id: file.id, content: format!("Error reading file: {}", e), kind: PreviewKind::UnSupported });
                    }
                }
            } else {
                let _ = tx.send(AppEvent::FilePreviewLoaded { file_id: file.id, content: String::new(), kind });
            }
        });
    }

    fn render_viewport_panel(&mut self, ctx: &egui::Context) {


        if self.viewport.active_file.is_some() {
            egui::SidePanel::right("viewport_panel")
                .resizable(true)
                .default_width(320.0)
                .frame(egui::Frame::none().fill(Colors::BG_PANEL).stroke(egui::Stroke::new(1.0, Colors::BORDER2)))
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        if ui.button(RichText::new("✕ CLOSE").color(Colors::TEXT_DIM)).clicked() {
                            self.viewport.active_file = None;
                        }
                    });
                    ui.add_space(8.0);
                    crate::views::viewport::show_viewport(ui, &mut self.viewport);
                });
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // UI helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn fmt_eta(secs: u64) -> String {
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m {}s", secs / 60, secs % 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }

    fn render_footer_progress(&self, ctx: &egui::Context) {
        let hashing_active = self.hashing_progress.1 > 0
            && self.hashing_progress.0 < self.hashing_progress.1;
        if !self.index_stats.scanning
            && self.embedding_progress.0 == self.embedding_progress.1
            && !hashing_active
        {
            return;
        }

        egui::TopBottomPanel::bottom("hud_footer_progress")
            .frame(egui::Frame::none().fill(Colors::BG_PANEL).inner_margin(egui::Margin::symmetric(10.0, 2.0)))
            .show(ctx, |ui| {
                ui.add_space(2.0);

                let progress = if self.index_stats.scanning {
                    if self.index_stats.scan_total > 0 {
                        (self.index_stats.scan_progress as f32 / self.index_stats.scan_total as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                } else if self.embedding_progress.1 > 0 && self.embedding_progress.0 < self.embedding_progress.1 {
                    (self.embedding_progress.0 as f32 / self.embedding_progress.1 as f32).clamp(0.0, 1.0)
                } else if self.hashing_progress.1 > 0 {
                    (self.hashing_progress.0 as f32 / self.hashing_progress.1 as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                // Brutalist Progress Bar — clamped so it never overflows past 100%
                let rect = ui.available_rect_before_wrap();
                let bar_rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.min.y + 4.0));
                ui.painter().rect_filled(bar_rect, 0.0, Color32::from_black_alpha(100));

                let fill_x = (bar_rect.min.x + bar_rect.width() * progress).min(bar_rect.max.x);
                let fill_rect = egui::Rect::from_min_max(bar_rect.min, egui::pos2(fill_x, bar_rect.max.y));
                ui.painter().rect_filled(fill_rect, 0.0, Colors::GREEN);

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let label = if self.index_stats.scanning {
                        format!("⬡ SCANNING: {:.1}%", progress * 100.0)
                    } else if self.embedding_progress.1 > 0 && self.embedding_progress.0 < self.embedding_progress.1 {
                        format!("⬡ EMBEDDING: {:.1}%", progress * 100.0)
                    } else {
                        format!(
                            "⬡ HASHING: {}/{} ({:.1}%)",
                            self.hashing_progress.0,
                            self.hashing_progress.1,
                            progress * 100.0
                        )
                    };

                    ui.label(RichText::new(label).color(Colors::TEXT).size(10.0).monospace());

                    // Rate label — scanning uses index_stats, hashing uses its own rate
                    if self.index_stats.scanning {
                        if let Some(rate) = self.index_stats.smoothed_files_per_sec {
                            ui.label(RichText::new(format!(" {:.1} f/s", rate)).color(Colors::TEXT_DIM).size(10.0).monospace());
                        }
                    } else if hashing_active {
                        if let Some(rate) = self.hashing_rate {
                            ui.label(RichText::new(format!(" {:.1} f/s", rate)).color(Colors::TEXT_DIM).size(10.0).monospace());
                        }
                    }

                    // ETA — scanning or hashing
                    if self.index_stats.scanning {
                        if let Some(eta) = self.index_stats.scan_eta_secs {
                            ui.label(RichText::new(format!(" ETA: {}", Self::fmt_eta(eta))).color(Colors::GREEN).size(10.0).monospace());
                        }
                    } else if hashing_active {
                        if let Some(eta) = self.hashing_eta_secs {
                            ui.label(RichText::new(format!(" ETA: {}", Self::fmt_eta(eta))).color(Colors::GREEN).size(10.0).monospace());
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Quick Type Stats
                        let mut sorted_types: Vec<_> = self.index_stats.type_counts.iter().collect();
                        sorted_types.sort_by(|a, b| b.1.cmp(a.1));
                        
                        let display = sorted_types.iter().take(4)
                            .map(|(ext, count)| format!("{}: {}", ext.to_uppercase(), count))
                            .collect::<Vec<_>>()
                            .join(" | ");
                        
                        ui.label(RichText::new(display).color(Colors::TEXT_DIM).size(9.0).monospace());
                    });
                });
                ui.add_space(4.0);
            });
    }

    fn push_toast(&mut self, t: Toast) { self.toasts.push(t); }
    fn set_status(&mut self, msg: impl Into<String>) { self.status_msg = msg.into(); }
    fn selected_ip(&self) -> Option<String> {
        self.selected_device().and_then(|d| d.primary_ip()).map(|s| s.to_string())
    }

    fn selected_device(&self) -> Option<&thegrid_core::TailscaleDevice> {
        self.selected_idx.and_then(|i| self.devices.get(i))
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
                    {
                        let watcher = self.runtime.file_watcher.lock().unwrap();
                        let cfg = self.runtime.config.lock().unwrap();
                        if watcher.is_some() {
                            ui.label(RichText::new("|").color(Colors::BORDER).size(9.0));
                            ui.label(
                                RichText::new(format!("WATCHING {}", cfg.watch_paths.len()))
                                    .color(if cfg.watch_paths.is_empty() { Colors::TEXT_MUTED } else { Colors::GREEN })
                                    .size(9.0)
                            );
                        }
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
            let rdp_user = self.rdp_usernames.get(device_id).map(|s| s.as_str())
                .or_else(|| if self.config.rdp_username.is_empty() { None } else { Some(&self.config.rdp_username) });
            
            let rdp_res_str = self.rdp_resolutions.get(device_id).cloned().unwrap_or_else(|| "FULLSCREEN".into());
            let res = RdpResolution::from_str(&rdp_res_str);
            
            match RdpLauncher::launch(ip, rdp_user, &res) {
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
            self.spawn_ping(ip.to_string(), device_id.to_string(), true);
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
                    self.spawn_send_file(device_id.to_string(), path, idx);
                }
            }
        }

        if actions.scan_remote {
            let is_local = device_id == self.config.device_name;
            if is_local {
                let drives = self.telemetry_cache.get(device_id)
                    .map(|t| t.capabilities.drives.clone())
                    .unwrap_or_default();
                for drive in drives {
                    self.spawn_index_directory(std::path::PathBuf::from(&drive.name), device_id.to_string(), device_id.to_string());
                }
            } else {
                self.push_toast(Toast::info("Remote scan request sent (stub)"));
            }
        }

        if let Some(filter) = actions.run_duplicate_scan {
            self.enqueue_local_full_drive_index();
            self.sync_all_nodes();
            self.set_status("Refreshing grid index and scanning duplicates across all drives/nodes...");
            self.spawn_duplicate_scan(filter);
        }

        if let Some(files) = actions.delete_duplicate_files {
            let filter = DuplicateScanFilter {
                min_size_bytes: self.file_manager.duplicate_min_size_mb.saturating_mul(1_048_576),
                include_extensions: self.file_manager.duplicate_ext_filter
                    .split(',')
                    .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                    .filter(|e| !e.is_empty())
                    .collect(),
                path_prefix: if self.file_manager.duplicate_path_filter.trim().is_empty() {
                    None
                } else {
                    Some(self.file_manager.duplicate_path_filter.trim().to_string())
                },
                device_id: None,
                exclude_system_paths: true,
                max_groups: self.file_manager.duplicate_max_groups,
            };
            self.spawn_delete_duplicate_files(files, filter);
        }

        if let Some(files) = actions.dedup_delete_files {
            self.spawn_rich_dedup_delete(files);
        }

        if actions.export_drive_buffer {
            self.spawn_export_drive_buffer();
        }

        if actions.upload_drive_buffer {
            let remote = if self.file_manager.drive_remote.trim().is_empty() {
                None
            } else {
                Some(self.file_manager.drive_remote.trim().to_string())
            };
            self.spawn_upload_drive_buffer(remote);
        }

        if let Some(paths) = actions.fm_delete {
            self.spawn_fm_delete(ip.to_string(), device_id.to_string(), paths);
        }

        if let Some(path) = actions.browse_remote {
            let is_local = self.devices.iter()
                .find(|d| d.id == device_id)
                .map(|d| d.name == self.config.device_name || d.hostname == self.config.device_name)
                .unwrap_or(false);
            if is_local {
                self.spawn_local_browse(device_id.to_string(), path);
            } else {
                self.spawn_browse_remote_directory(ip.to_string(), device_id.to_string(), path);
            }
        }

        if let Some(filename) = actions.download_file {
            self.transfer_log.push(TransferLogEntry::info(format!("Downloading {}...", filename)));
            self.spawn_download_file(ip.to_string(), filename);
        }

        if let Some(path) = actions.preview_remote {
            self.spawn_preview_remote_file(ip.to_string(), path);
        }

        if let Some(path) = actions.download_remote_file {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            self.transfer_log.push(TransferLogEntry::info(format!("Downloading {}...", name)));
            self.spawn_download_remote_file_anywhere(ip.to_string(), path);
        }

        if let Some((dt, model, url)) = actions.update_remote_config {
            self.spawn_update_remote_config(ip.to_string(), device_id.to_string(), dt, model, url);
        }

        if actions.create_terminal {
            self.spawn_create_terminal_session(ip.to_string(), device_id.to_string());
        }

        if actions.launch_scrcpy {
            self.push_toast(Toast::info("Initializing Screen Mirroring (scrcpy)..."));
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

        if actions.enable_rdp {
            self.spawn_enable_rdp(ip.to_string(), device_id.to_string());
        }

        if actions.launch_ssh {
            // Tailscale SSH: ssh <ip>
            #[cfg(target_os = "windows")]
            let _ = std::process::Command::new("cmd")
                .args(&["/C", "start", "ssh", ip])
                .spawn();
            
            #[cfg(not(target_os = "windows"))]
            let _ = std::process::Command::new("ssh").arg(ip).spawn();
        }


    }

    // ── Remote Terminal Spawners ─────────────────────────────────────────────

    fn spawn_create_terminal_session(&mut self, ip: String, device_id: String) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key) {
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
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();

        std::thread::spawn(move || {
            if let Ok(client) = AgentClient::new(&ip, port, api_key) {
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

    fn spawn_preview_remote_file(&mut self, ip: String, path: std::path::PathBuf) {
        let port = self.config.agent_port;
        let api_key = self.config.api_key.clone();
        let tx = self.event_tx.clone();

        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key) {
                Ok(client) => match client.preview_file(&path.to_string_lossy()) {
                    Ok(bytes) => {
                        let _ = tx.send(AppEvent::AgentFilePreviewLoaded(bytes));
                    }
                    Err(e) => {
                        let content = format!("Failed to read preview: {}", e);
                        let _ = tx.send(AppEvent::AgentFilePreviewLoaded(content.into_bytes()));
                    }
                },
                Err(e) => {
                    let content = format!("Failed to connect: {}", e);
                    let _ = tx.send(AppEvent::AgentFilePreviewLoaded(content.into_bytes()));
                }
            }
        });
    }


    fn spawn_launch_scrcpy(&mut self, ip: String) {
        if !Self::command_exists("adb") {
            self.push_toast(Toast::err("ADB not found. Install Android Platform Tools and add adb to PATH."));
            self.set_status("ADB missing on this machine");
            return;
        }
        if !Self::command_exists("scrcpy") {
            self.push_toast(Toast::err("scrcpy not found. Install scrcpy and add it to PATH."));
            self.set_status("scrcpy missing on this machine");
            return;
        }

        log::info!("Checking for USB-connected devices for high-performance mirroring (BPW)...");
        
        // 1. Try to find a USB device first
        let output = std::process::Command::new("adb").arg("devices").arg("-l").output();
        let mut usb_serial = None;
        if let Ok(out) = output {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines().skip(1) {
                if line.contains("usb:") && !line.is_empty() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if !parts.is_empty() {
                        usb_serial = Some(parts[0].to_string());
                        break; 
                    }
                }
            }
        }

        if let Some(serial) = usb_serial {
            log::info!("BPW: Found USB device {}. Launching scrcpy via USB...", serial);
            let _ = std::process::Command::new("scrcpy")
                .arg("-s")
                .arg(serial)
                .spawn();
            self.push_toast(Toast::ok("BPW: Mirroring via USB"));
        } else {
            log::info!("BPW: No USB device found. Falling back to Network Mirroring for {}", ip);
            let api_key = self.config.api_key.clone();
            let _ = self.event_tx.send(AppEvent::EnableAdb { ip, api_key });
        }
    }

    pub fn get_node_status(&self, device_id: &str) -> NodeStatus {
        let device = self.devices.iter().find(|d| d.id == device_id);
        let is_reachable = device.map(|d| d.is_likely_online()).unwrap_or(false);
        if !is_reachable { return NodeStatus::Offline; }
        
        let last_poll = self.telemetry_last_poll.get(device_id);
        // If we've heard from the agent in the last 2.5 minutes, consider it active
        let is_active = last_poll.map(|t| t.elapsed().as_secs() < 150).unwrap_or(false);
        
        if is_active { NodeStatus::GridActive } else { NodeStatus::Reachable }
    }

    /// BPW: Identify the optimal IP for reaching a device (prefers LAN/Direct)
    fn find_best_ip(&self, device_id: &str) -> String {
        let device = self.devices.iter().find(|d| d.id == device_id);
        let tailscale_ip = device.and_then(|d| d.primary_ip()).map(|s| s.to_string());
        
        if let Some(telemetry) = self.telemetry_cache.get(device_id) {
            // Get local machine's IPs from its own telemetry
            let local_node_id = self.devices.iter().find(|d| d.name == self.config.device_name).map(|d| &d.id);
            let my_ips = local_node_id.and_then(|id| self.telemetry_cache.get(id)).map(|t| &t.local_ips);
            
            if let Some(my_ips) = my_ips {
                for remote_ip in &telemetry.local_ips {
                    for my_ip in my_ips {
                        // Simple subnet check: match first 3 octets for 24-bit mask
                        let r_parts: Vec<&str> = remote_ip.split('.').collect();
                        let m_parts: Vec<&str> = my_ip.split('.').collect();
                        if r_parts.len() == 4 && m_parts.len() == 4 && r_parts[..3] == m_parts[..3] {
                            log::info!("BPW: Direct LAN connection detected targeting {} at {}", device_id, remote_ip);
                            return remote_ip.clone();
                        }
                    }
                }
            }
        }
        
        tailscale_ip.unwrap_or_else(|| "127.0.0.1".into())
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
            if let Some(_ip) = self.selected_ip() {
                for path in dropped {
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let idx  = self.file_queue.len();
                    self.file_queue.push(FileQueueItem {
                        path: path.clone(), name: name.clone(), size,
                        status: FileTransferStatus::Sending,
                    });
                    self.transfer_log.push(TransferLogEntry::info(format!("Sending {}...", name)));
                    if let Some(d) = self.selected_device() {
                        self.spawn_send_file(d.id.clone(), path, idx);
                    }
                }
            }
        }

        // 5. Periodic telemetry and ping refresh (every 30s)
        let mut ping_targets = Vec::new(); // (ip, device_id)
        let mut sync_targets = Vec::new(); // (device_id, ip, hostname)
        let mut local_telemetry_device_id: Option<String> = None;

        // Recover from a stuck local telemetry worker so updates can resume.
        if self.local_telemetry_pending {
            if let Some(started) = self.local_telemetry_pending_since {
                if started.elapsed().as_secs() > 20 {
                    self.local_telemetry_pending = false;
                    self.local_telemetry_pending_since = None;
                    self.set_status("Local telemetry timeout; retrying...".to_string());
                }
            }
        }

        for d in &self.devices {
            if let Some(ip) = d.primary_ip() {
                let last_telemetry_poll = self.telemetry_last_poll.get(&d.id);
                
                // Tiered polling: 15s for selected/local, 60s for active ones, 300s for idle/offline
                let is_selected = self.selected_idx.and_then(|i| self.devices.get(i)).map(|sd| sd.id == d.id).unwrap_or(false);
                let is_local = self.is_local_device(d);
                
                let interval = if is_selected || is_local {
                    15 
                } else if self.get_node_status(&d.id) == NodeStatus::GridActive {
                    60
                } else {
                    300
                };

                let needs_poll = last_telemetry_poll.map(|t| t.elapsed().as_secs() > interval).unwrap_or(true);
                
                if needs_poll {
                    if is_local {
                        if !self.local_telemetry_pending {
                            local_telemetry_device_id = Some(d.id.clone());
                        }
                    } else {
                        let should_probe = self.ping_last_poll
                            .get(&d.id)
                            .map(|t| t.elapsed().as_secs() > interval)
                            .unwrap_or(true);
                        if should_probe {
                            ping_targets.push((ip.to_string(), d.id.clone()));
                        }
                    }
                }

                if !is_local {
                    let sync_interval = if is_selected { 20 } else if self.get_node_status(&d.id) == NodeStatus::GridActive { 75 } else { 180 };
                    let sync_due = self.sync_last_poll
                        .get(&d.id)
                        .map(|t| t.elapsed().as_secs() > sync_interval)
                        .unwrap_or(true);
                    if sync_due {
                        sync_targets.push((d.id.clone(), ip.to_string(), d.hostname.clone()));
                    }
                }
            }
        }

        if let Some(local_device_id) = local_telemetry_device_id {
            self.spawn_collect_local_telemetry(local_device_id);
        }
        for (ip, id) in ping_targets {
            self.spawn_ping(ip, id, false);
        }
        for (device_id, ip, hostname) in sync_targets {
            self.spawn_sync_node_if_due(device_id, ip, hostname, 0);
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
                self.start_initial_watch_scans();
                self.start_release_check();
                self.render_titlebar(ctx);
                self.render_statusbar(ctx);
                self.render_footer_progress(ctx);

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
                        let devices_with_status: Vec<_> = self.devices.iter().map(|d| {
                            (d.clone(), self.get_node_status(&d.id))
                        }).collect();

                        device_clicked = crate::views::dashboard::render_device_panel(
                            ui,
                            &devices_with_status,
                            &self.device_display_states,
                            &telemetry_snap,
                            self.selected_idx,
                            &mut self.selected_node_ids,
                            &self.config.projects,
                            &self.config.categories,
                            &self.config.smart_rules,
                            &mut self.file_manager.active_rule,
                            &mut self.device_filter,
                            &mut needs_refresh,
                            &self.local_hostname,
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
                        self.remote_url_edit = String::new();
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

                        // ── Cluster View vs Single Detail View ────────────────────────
                        let is_cluster = self.selected_node_ids.len() > 1;

                        if is_cluster {
                            let cluster_devices: Vec<TailscaleDevice> = self.devices.iter()
                                .filter(|d| self.selected_node_ids.contains(&d.id))
                                .cloned()
                                .collect();
                            let active_rule = self.file_manager.active_rule.as_ref()
                                .and_then(|id| self.config.smart_rules.iter().find(|r| &r.id == id));

                            let cluster_actions = crate::views::dashboard::render_cluster_view(
                                ui,
                                &cluster_devices,
                                &telemetry_snap,
                                &mut self.cluster_paths,
                                &self.cluster_files,
                                &self.local_hostname,
                                active_rule,
                            );

                            if let Some((node_id, path)) = cluster_actions.load_node_path {
                                self.spawn_load_cluster_path(node_id, path);
                            }
                        } else {
                            // Single device detail view (Existing logic)
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
                            let watch_snap   = self.runtime.config.lock().unwrap().watch_paths.clone();
                            let telem_snap   = telemetry_snap.get(&device.id).cloned();
                            let drive_manifest_snap = self.file_manager.drive_last_manifest.clone();
                            let status = self.get_node_status(&device.id);

                            let rdp_user = self.rdp_usernames.entry(device.id.clone())
                                .or_insert_with(|| self.config.rdp_username.clone());
                            let rdp_res  = self.rdp_resolutions.entry(device.id.clone())
                                .or_insert_with(|| "FULLSCREEN".into());

                            let mut detail = DetailState {
                                device:         &device,
                                active_tab:     &mut self.active_tab,
                                rdp_username:   rdp_user,
                                rdp_resolution: rdp_res,
                                clip_out:       &mut self.clip_out,
                                clip_inbox:     &clip_snap,
                                file_queue:     &queue_snap,
                                remote_files:   &remote_snap,
                                transfer_log:   &log_snap,
                                is_tg_agent:    self.is_tg_agent,
                                watch_paths:    &watch_snap,
                                telemetry:      telem_snap.as_ref(),
                                smart_rules:    &self.config.smart_rules,
                                _current_remote_path: &mut self.current_remote_path,
                                file_manager: &mut self.file_manager,
                                remote_model_edit: &mut self.remote_model_edit,
                                remote_url_edit: &mut self.remote_url_edit,
                                terminal_view: self.terminal_sessions.get_mut(&device.id),
                                local_device_name: &self.local_hostname,
                                status,
                                duplicate_groups: &self.duplicate_groups,
                                duplicate_last_scan: self.duplicate_last_scan,
                                hashing_progress: self.hashing_progress,
                                drive_last_manifest: drive_manifest_snap.as_ref(),
                                grid_scan_progress: &self.grid_scan_progress,
                                cloud_pipeline_progress: &self.cloud_pipeline_progress,
                                node_crosscheck: &self.node_crosscheck,
                                rich_duplicate_groups: &self.rich_duplicate_groups,
                                dedup_review_state: &mut self.dedup_review_state,
                                local_device_id: &self.config.device_name,
                            };

                            // Pass timeline state into the render
                            let actions = crate::views::dashboard::render_detail_panel_with_timeline(
                                ui,
                                &mut detail,
                                &mut self.timeline,
                                &self.index_stats,
                                &mut self.telemetry_tree,
                                &mut self.telemetry_band_height,
                            );

                            if let Some(ip) = device.primary_ip().map(|s| s.to_string()) {
                                self.handle_detail_actions(actions, &ip, &device.id);
                            }
                        } else {
                            crate::views::dashboard::render_empty_state(ui);
                        }
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

                             // Live update watcher
                            let mut watcher_lock = self.runtime.file_watcher.lock().unwrap();
                            if let Some(fw) = &mut *watcher_lock {
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
                            drop(watcher_lock);
                            
                            let needs_restart = new_config.api_key != self.config.api_key || new_config.agent_port != self.config.agent_port;
                            
                            self.config = new_config.clone();
                            // Keep runtime config in sync
                            *self.runtime.config.lock().unwrap() = new_config;
                            
                            if needs_restart {
                                self.runtime.restart_services();
                            }
                            
                            self.spawn_load_devices();
                        }
                        Err(e) => self.push_toast(Toast::err(format!("Save failed: {}", e))),
                    }
                }

                // Search overlay (Ctrl+F)
                let search_action = crate::views::search::render(
                    ctx, &mut self.search, &self.index_stats,
                    self.selected_idx.and_then(|idx| self.devices.get(idx)).map(|d| (d.id.clone(), d.display_name().to_string())),
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
                if let Some(result) = search_action.preview_result {
                    let _ = self.event_tx.send(AppEvent::RequestFilePreview(result));
                }
                // Idle Detection
                let now = std::time::Instant::now();
                if ctx.input(|i| i.pointer.any_click() || i.pointer.any_down() || !i.events.is_empty()) {
                    self.last_input_at = now;
                    if self.idle_notified {
                        self.idle_notified = false;
                        log::info!("User returned from idle, pausing background tasks");
                    }
                }

                if !self.idle_notified && now.duration_since(self.last_input_at).as_secs() > 600 {
                    self.idle_notified = true;
                    log::info!("System idled for 10m, requesting background tasks resume");
                    let _ = self.event_tx.send(AppEvent::RequestIdleWork);
                }

                self.render_viewport_panel(ctx);

                self.render_toasts(ctx);

                // --- Phase 3: Periodic Sync ---
                if self.mesh_sync_last_at.elapsed().as_secs() > 120 {
                    self.mesh_sync_last_at = std::time::Instant::now();
                    self.sync_all_nodes();
                }
            }
        }
    }
}
