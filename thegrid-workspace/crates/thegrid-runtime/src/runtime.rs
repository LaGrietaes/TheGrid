use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::collections::HashSet;
use base64::Engine;
use anyhow::Result;
use rusqlite::params;

use thegrid_core::{
    fingerprint_file, match_rules, should_skip_dir, should_skip_path, AppEvent, Config, Database,
    DetectionSourceDistribution, FileChange, FileWatcher, SyncHealthMetrics, TailscaleDevice,
    models::ComputeTaskType,
};
use thegrid_net::{AgentClient, AgentServer, DriveClient, TailscaleClient, TermuxAgent, WolSentry};
use thegrid_ai::{SemanticSearch, EmbeddingProvider, AiNodeDetector};
use crate::ComputeRouter;

fn apply_automation_rules_for_file(
    db: &Database,
    cfg: &Config,
    file_id: i64,
    path: &std::path::Path,
    size: u64,
    modified: Option<i64>,
) {
    if let Ok(rules) = db.get_rules() {
        let user_rules: Vec<_> = rules.into_iter().map(|r| {
            thegrid_core::models::UserRule {
                id: r.0,
                name: r.1,
                pattern: r.2,
                project: r.3,
                tag: r.4,
                is_active: r.5,
            }
        }).collect();

        let matches = match_rules(path, &user_rules, file_id);
        for m in matches {
            let _ = db.add_file_tag(file_id, m.tag.as_deref(), m.project.as_deref(), false);
        }
    }

    let ext = path.extension()
        .map(|e| e.to_string_lossy().to_string().to_lowercase())
        .unwrap_or_default();

    for rule in &cfg.smart_rules {
        let mut matched = true;
        let mut project_output: Option<String> = None;
        let mut category_output: Option<String> = None;

        for filter in &rule.filters {
            match filter {
                thegrid_core::models::SmartFilterType::Extension(expected) => {
                    let expected = expected.trim_start_matches('.').to_lowercase();
                    if ext != expected {
                        matched = false;
                        break;
                    }
                }
                thegrid_core::models::SmartFilterType::MinSize(min) => {
                    if size < *min {
                        matched = false;
                        break;
                    }
                }
                thegrid_core::models::SmartFilterType::MaxSize(max) => {
                    if size > *max {
                        matched = false;
                        break;
                    }
                }
                thegrid_core::models::SmartFilterType::ModifiedAfter(dt) => {
                    let ts = modified.unwrap_or(0);
                    if ts <= dt.timestamp() {
                        matched = false;
                        break;
                    }
                }
                thegrid_core::models::SmartFilterType::ModifiedBefore(dt) => {
                    let ts = modified.unwrap_or(i64::MAX);
                    if ts >= dt.timestamp() {
                        matched = false;
                        break;
                    }
                }
                thegrid_core::models::SmartFilterType::Project(project_id) => {
                    project_output = Some(project_id.clone());
                }
                thegrid_core::models::SmartFilterType::Category(category_id) => {
                    category_output = Some(category_id.clone());
                }
            }
        }

        if matched {
            let rule_tag = format!("rule:{}", rule.id);
            let category_tag = category_output.as_ref().map(|c| format!("category:{}", c));
            let _ = db.add_file_tag(file_id, Some(&rule_tag), project_output.as_deref(), false);
            if let Some(cat_tag) = category_tag.as_deref() {
                let _ = db.add_file_tag(file_id, Some(cat_tag), project_output.as_deref(), false);
            }
        }
    }
}

pub struct AppRuntime {
    pub config:       Arc<Mutex<Config>>,
    pub db:           Arc<Mutex<Database>>,
    pub db_path:      PathBuf,
    pub event_tx:     mpsc::Sender<AppEvent>,
    
    // Services
    pub file_watcher:    Arc<Mutex<Option<FileWatcher>>>,
    pub semantic_search: Arc<Mutex<Option<SemanticSearch>>>,
    
    // Remote AI Capabilities (device_id -> ip)
    pub remote_ai_nodes: Arc<Mutex<std::collections::HashMap<String, String>>>,
    pub termux_agent: Arc<Mutex<Option<TermuxAgent>>>,
    
    // State
    pub is_ai_node: bool,
    pub media_analyzer: Arc<Mutex<Option<Arc<dyn thegrid_ai::MediaAnalyzer>>>>,
    pub agent_shutdown: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    pub hash_worker_running: Arc<AtomicBool>,
    pub embedding_worker_running: Arc<AtomicBool>,
    pub media_worker_running: Arc<AtomicBool>,
    pub ui_priority_mode: Arc<AtomicBool>,
    pub sync_health: Arc<Mutex<std::collections::HashMap<String, SyncHealthMetrics>>>,

    // Compute delegation
    pub tailscale_peers: Arc<Mutex<Vec<TailscaleDevice>>>,
    pub compute_router: Arc<ComputeRouter>,
}

#[derive(Debug, Clone, Default)]
struct SearchMediaFilters {
    in_focus: Option<bool>,
    min_quality: Option<f32>,
    min_focus_score: Option<f32>,
    min_megapixels: Option<f32>,
    camera_contains: Option<String>,
    lens_contains: Option<String>,
    min_iso: Option<u32>,
    max_iso: Option<u32>,
    min_aperture: Option<f32>,
    max_aperture: Option<f32>,
    min_focal_mm: Option<f32>,
    max_focal_mm: Option<f32>,
    captured_after: Option<String>,
    captured_before: Option<String>,
    require_gps: Option<bool>,
    min_rating: Option<u8>,
    pick_flag: Option<String>,
}

impl SearchMediaFilters {
    fn any(&self) -> bool {
        self.in_focus.is_some()
            || self.min_quality.is_some()
            || self.min_focus_score.is_some()
            || self.min_megapixels.is_some()
            || self.camera_contains.is_some()
            || self.lens_contains.is_some()
            || self.min_iso.is_some()
            || self.max_iso.is_some()
            || self.min_aperture.is_some()
            || self.max_aperture.is_some()
            || self.min_focal_mm.is_some()
            || self.max_focal_mm.is_some()
            || self.captured_after.is_some()
            || self.captured_before.is_some()
            || self.require_gps.is_some()
            || self.min_rating.is_some()
            || self.pick_flag.is_some()
    }
}

impl AppRuntime {
    fn parse_media_processing_mode(raw: &str) -> thegrid_ai::MediaProcessingMode {
        match raw.trim().to_ascii_lowercase().as_str() {
            "cpu" => thegrid_ai::MediaProcessingMode::Cpu,
            "gpu" | "dedicated_gpu" | "dedicated-gpu" | "nvidia" => {
                thegrid_ai::MediaProcessingMode::DedicatedGpu
            }
            _ => thegrid_ai::MediaProcessingMode::Auto,
        }
    }

    fn parse_media_search_filters(raw_query: &str) -> (String, SearchMediaFilters) {
        let mut filters = SearchMediaFilters::default();
        let mut kept = Vec::new();

        for token in raw_query.split_whitespace() {
            let lower = token.to_ascii_lowercase();

            if let Some(v) = lower.strip_prefix("focus:") {
                match v {
                    "in" | "true" | "sharp" => {
                        filters.in_focus = Some(true);
                        continue;
                    }
                    "out" | "false" | "blur" | "blurry" => {
                        filters.in_focus = Some(false);
                        continue;
                    }
                    _ => {}
                }
            }

            if let Some(v) = lower.strip_prefix("quality>=") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_quality = Some(n.clamp(0.0, 1.0));
                    continue;
                }
            }
            if let Some(v) = lower.strip_prefix("quality>") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_quality = Some(n.clamp(0.0, 1.0));
                    continue;
                }
            }

            if let Some(v) = lower.strip_prefix("focusscore>=") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_focus_score = Some(n.clamp(0.0, 1.0));
                    continue;
                }
            }
            if let Some(v) = lower.strip_prefix("focusscore>") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_focus_score = Some(n.clamp(0.0, 1.0));
                    continue;
                }
            }

            if let Some(v) = lower.strip_prefix("mp>=") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_megapixels = Some(n.max(0.0));
                    continue;
                }
            }
            if let Some(v) = lower.strip_prefix("mp>") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_megapixels = Some(n.max(0.0));
                    continue;
                }
            }

            if let Some(v) = token.strip_prefix("camera:") {
                let v = v.trim();
                if !v.is_empty() {
                    filters.camera_contains = Some(v.to_string());
                    continue;
                }
            }

            if let Some(v) = token.strip_prefix("lens:") {
                let v = v.trim();
                if !v.is_empty() {
                    filters.lens_contains = Some(v.to_string());
                    continue;
                }
            }

            if let Some(v) = lower.strip_prefix("iso>=") {
                if let Ok(n) = v.parse::<u32>() {
                    filters.min_iso = Some(n);
                    continue;
                }
            }
            if let Some(v) = lower.strip_prefix("iso<=") {
                if let Ok(n) = v.parse::<u32>() {
                    filters.max_iso = Some(n);
                    continue;
                }
            }

            if let Some(v) = lower.strip_prefix("aperture>=") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_aperture = Some(n.max(0.0));
                    continue;
                }
            }
            if let Some(v) = lower.strip_prefix("aperture<=") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.max_aperture = Some(n.max(0.0));
                    continue;
                }
            }

            if let Some(v) = lower.strip_prefix("focal>=") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.min_focal_mm = Some(n.max(0.0));
                    continue;
                }
            }
            if let Some(v) = lower.strip_prefix("focal<=") {
                if let Ok(n) = v.parse::<f32>() {
                    filters.max_focal_mm = Some(n.max(0.0));
                    continue;
                }
            }

            if let Some(v) = token.strip_prefix("captured>=") {
                let val = v.trim();
                if !val.is_empty() {
                    filters.captured_after = Some(val.to_string());
                    continue;
                }
            }
            if let Some(v) = token.strip_prefix("captured<=") {
                let val = v.trim();
                if !val.is_empty() {
                    filters.captured_before = Some(val.to_string());
                    continue;
                }
            }

            if lower == "gps:true" || lower == "gps:yes" || lower == "gps:on" {
                filters.require_gps = Some(true);
                continue;
            }
            if lower == "gps:false" || lower == "gps:no" || lower == "gps:off" {
                filters.require_gps = Some(false);
                continue;
            }

            if let Some(v) = lower.strip_prefix("rating>=") {
                if let Ok(n) = v.parse::<u8>() {
                    filters.min_rating = Some(n.min(5));
                    continue;
                }
            }

            if let Some(v) = lower.strip_prefix("pick:") {
                match v {
                    "keep" | "pick" | "selected" => {
                        filters.pick_flag = Some("pick".to_string());
                        continue;
                    }
                    "reject" | "discard" | "drop" => {
                        filters.pick_flag = Some("reject".to_string());
                        continue;
                    }
                    "none" | "unreviewed" => {
                        filters.pick_flag = Some("none".to_string());
                        continue;
                    }
                    _ => {}
                }
            }

            kept.push(token.to_string());
        }

        (kept.join(" "), filters)
    }

    fn parse_agent_target(endpoint: &str, fallback_port: u16) -> Option<(String, u16)> {
        let trimmed = endpoint.trim();
        let without_scheme = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .unwrap_or(trimmed);
        let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
        if host_port.is_empty() {
            return None;
        }

        if let Some((host, port_str)) = host_port.rsplit_once(':') {
            let port = port_str.parse::<u16>().ok().unwrap_or(fallback_port);
            Some((host.to_string(), port))
        } else {
            Some((host_port.to_string(), fallback_port))
        }
    }

    fn is_offloadable_image(path: &std::path::Path) -> bool {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp" | "tif" | "tiff")
    }

    fn encode_image_as_data_url(path: &std::path::Path, max_bytes: usize) -> Option<String> {
        let bytes = std::fs::read(path).ok()?;
        if bytes.is_empty() || bytes.len() > max_bytes {
            return None;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        Some(format!("data:application/octet-stream;base64,{}", b64))
    }

    fn request_remote_image_analysis(
        api_key: &str,
        ip: &str,
        port: u16,
        file_id: i64,
        path: &std::path::Path,
        urgent: bool,
    ) -> Option<String> {
        if !Self::is_offloadable_image(path) {
            return None;
        }

        // Keep request payload bounded for responsiveness over mesh links.
        let payload_url = Self::encode_image_as_data_url(path, 8 * 1024 * 1024)?;
        let client = AgentClient::new(ip, port, api_key.to_string()).ok()?;

        let task_id = format!(
            "media-{}-{}",
            file_id,
            chrono::Utc::now().timestamp_millis()
        );

        let req = thegrid_core::models::ComputeTaskRequest {
            task_id: task_id.clone(),
            task_type: thegrid_core::models::ComputeTaskType::ImageEmbedding,
            requester_device_id: "thegrid-runtime".to_string(),
            requester_callback_url: String::new(),
            payload: thegrid_core::models::ComputePayload::ImageEmbed { file_url: payload_url },
            priority: if urgent { 10 } else { 6 },
            deadline_secs: Some(if urgent { 45 } else { 60 }),
        };

        let receipt = client.post_compute_request(&req).ok()?;
        if !receipt.accepted {
            return None;
        }

        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(50) {
            let p = client.get_compute_progress(&task_id).ok()?;
            match p.state {
                thegrid_core::models::ComputeTaskState::Done => {
                    let _ = client.ack_compute_result(&task_id);
                    return p.result_uri;
                }
                thegrid_core::models::ComputeTaskState::Failed
                | thegrid_core::models::ComputeTaskState::Cancelled => {
                    return None;
                }
                _ => {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
        }
        None
    }

    pub fn new(config: Config, event_tx: mpsc::Sender<AppEvent>) -> Result<Self> {
        let db_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("thegrid")
            .join("index.db");

        let db = match Database::open(&db_path) {
            Ok(d) => Arc::new(Mutex::new(d)),
            Err(e) => {
                let msg = format!("DB open failed ({}), using in-memory fallback", e);
                log::error!("{}", msg);
                let _ = event_tx.send(AppEvent::Status(format!("db_error:{}", msg)));
                Arc::new(Mutex::new(Database::open(":memory:")?))
            }
        };

        // Initialize services
        let (file_watcher, _watch_paths) = match FileWatcher::new(event_tx.clone()) {
            Ok(mut fw) => {
                let wp = config.watch_paths.clone();
                for path in &wp {
                    let _ = fw.watch(path.clone());
                }
                (Arc::new(Mutex::new(Some(fw))), wp)
            }
            Err(e) => {
                log::warn!("FileWatcher unavailable: {}", e);
                (Arc::new(Mutex::new(None)), vec![])
            }
        };

        let ai_node_detector = AiNodeDetector::new();
        let is_ai_node = ai_node_detector.is_ai_node();

        let config_arc = Arc::new(Mutex::new(config));
        let compute_router = Arc::new(ComputeRouter::new(
            Arc::clone(&config_arc),
            event_tx.clone(),
        ));

        let runtime = Self {
            config: config_arc,
            db,
            db_path,
            event_tx,
            file_watcher,
            semantic_search: Arc::new(Mutex::new(None)),
            remote_ai_nodes: Arc::new(Mutex::new(std::collections::HashMap::new())),
            termux_agent: Arc::new(Mutex::new(None)),
            is_ai_node,
            media_analyzer: Arc::new(Mutex::new(None)),
            agent_shutdown: Arc::new(Mutex::new(None)),
            hash_worker_running: Arc::new(AtomicBool::new(false)),
            embedding_worker_running: Arc::new(AtomicBool::new(false)),
            media_worker_running: Arc::new(AtomicBool::new(false)),
            ui_priority_mode: Arc::new(AtomicBool::new(false)),
            sync_health: Arc::new(Mutex::new(std::collections::HashMap::new())),
            tailscale_peers: Arc::new(Mutex::new(Vec::new())),
            compute_router,
        };

        Ok(runtime)
    }

    /// Called by the GUI whenever Tailscale peer list is refreshed so that the
    /// compute router always has a current view of available nodes.
    pub fn update_tailscale_peers(&self, peers: Vec<TailscaleDevice>) {
        let mut lock = self.tailscale_peers.lock().unwrap();
        *lock = peers;
    }

    /// When enabled, interactive UI screens (e.g. Media Ingest) get priority.
    /// Background workers reduce batch sizes and parallelism to keep the app responsive.
    pub fn set_ui_priority_mode(&self, enabled: bool) {
        self.ui_priority_mode.store(enabled, Ordering::Relaxed);
    }

    /// Periodically re-fetch Tailscale devices in the background so that
    /// the compute router works even without an active GUI (e.g., node mode).
    pub fn spawn_peer_refresh_loop(&self) {
        let api_key = self.config.lock().unwrap().api_key.clone();
        let peers_slot = Arc::clone(&self.tailscale_peers);
        std::thread::Builder::new()
            .name("thegrid-peer-refresh".into())
            .spawn(move || {
                loop {
                    match TailscaleClient::new(api_key.clone()).and_then(|c| c.fetch_devices()) {
                        Ok(devices) => {
                            let n = devices.len();
                            let mut lock = peers_slot.lock().unwrap();
                            *lock = devices;
                            drop(lock);
                            log::debug!("[Runtime] Peer refresh: {} device(s) known", n);
                        }
                        Err(e) => {
                            log::debug!("[Runtime] Peer refresh failed: {}", e);
                        }
                    }
                    // Refresh every 2 minutes — cheap since Tailscale API caches for 5 min
                    std::thread::sleep(std::time::Duration::from_secs(120));
                }
            })
            .ok();
    }

    pub fn start_services(&self) {
        let _ = self.event_tx.send(AppEvent::Status("startup_phase:Preparing local services...".to_string()));
        let (p, k, c) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone(), cfg.clone())
        };

        let _ = self.event_tx.send(AppEvent::Status(format!(
            "security_gates:file_access={},terminal_access={},ai_access={},remote_control={},rdp={}",
            c.enable_file_access,
            c.enable_terminal_access,
            c.enable_ai_access,
            c.enable_remote_control,
            c.enable_rdp
        )));

        // Start agent server
        let _ = self.event_tx.send(AppEvent::Status(format!("startup_phase:Starting local agent on port {}...", p)));
        let transfers_dir = c.effective_transfers_dir();
        let mut server = AgentServer::new(
            p,
            k.clone(),
            transfers_dir,
            self.event_tx.clone(),
            Arc::clone(&self.config)
        );

        if !k.trim().is_empty() {
            if let Ok(ts_client) = TailscaleClient::new(k) {
                server = server.with_tailscale(Arc::new(ts_client));
            }
        }

        // Detect & setup Termux node via USB-C OTG in background so GUI startup stays responsive.
        let termux_slot = Arc::clone(&self.termux_agent);
        let termux_event_tx = self.event_tx.clone();
        std::thread::spawn(move || {
            if let Some(agent) = thegrid_net::setup_termux_agent() {
                let method = agent.connection_method();
                let endpoint = agent.endpoint().to_string();
                {
                    let mut slot = termux_slot.lock().unwrap();
                    *slot = Some(agent);
                }
                log::info!("[Runtime] Tablet (Android) available via {} at {}", method, endpoint);
                let _ = termux_event_tx.send(AppEvent::Status(format!(
                    "termux_ready:{}:{}",
                    method,
                    endpoint
                )));
            } else {
                let mut slot = termux_slot.lock().unwrap();
                *slot = None;
                log::debug!("[Runtime] No Termux/Android device available");
            }
        });

        // Start AI if capable OR if provider URL is set (which overrides local specs)
        let has_remote_provider = {
            let cfg = self.config.lock().unwrap();
            cfg.ai_provider_url.is_some()
        };

        if self.is_ai_node || has_remote_provider {
            let _ = self.event_tx.send(AppEvent::Status("startup_phase:Initializing semantic services...".to_string()));
            self.spawn_semantic_initializer();
            self.spawn_embedding_worker();
        }

        // Phase 4: Media AI — enabled by default and runs on CPU/GPU depending on host capabilities.
        let media_ai_enabled = std::env::var("THEGRID_ENABLE_MEDIA_AI")
            .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(true);

        if media_ai_enabled {
            let _ = self.event_tx.send(AppEvent::Status("startup_phase:Initializing media analyzer...".to_string()));
            let mode_raw = std::env::var("THEGRID_MEDIA_MODE").ok().unwrap_or_else(|| {
                self.config
                    .lock()
                    .map(|c| c.media_processing_mode.clone())
                    .unwrap_or_else(|_| "auto".to_string())
            });
            let media_mode = Self::parse_media_processing_mode(&mode_raw);

            match thegrid_ai::CudaMediaAnalyzer::new_with_mode(media_mode) {
                Ok(analyzer) => {
                    let mut lock = self.media_analyzer.lock().unwrap();
                    *lock = Some(Arc::new(analyzer));
                    drop(lock);
                    log::info!(
                        "[Runtime] Media analyzer ready (mode={}) — starting background worker",
                        media_mode.as_str()
                    );
                    self.spawn_media_analyzer_worker();
                }
                Err(e) => {
                    log::warn!("[Runtime] Media analyzer unavailable: {}", e);
                    let _ = self.event_tx.send(AppEvent::Status(format!(
                        "Media analyzer unavailable: {}",
                        e
                    )));
                }
            }
        } else {
            log::info!("[Runtime] Media analyzer disabled (set THEGRID_ENABLE_MEDIA_AI=0 to disable)");
        }

        // Background indexing helpers + peer discovery for compute delegation
        let _ = self.event_tx.send(AppEvent::Status("startup_phase:Starting background workers...".to_string()));
        self.spawn_peer_refresh_loop();
        self.spawn_hashing_worker();

        let shutdown_handle = server.shutdown_handle();
        match server.spawn() {
            Ok(()) => {
                let mut shutdown_lock = self.agent_shutdown.lock().unwrap();
                *shutdown_lock = Some(shutdown_handle);
            }
            Err(e) => {
                log::error!("[Runtime] Agent startup failed on port {}: {}", p, e);
                let _ = self.event_tx.send(AppEvent::Status(format!(
                    "agent_start_failed:{}:{}",
                    p,
                    e
                )));
            }
        }

        let _ = self.event_tx.send(AppEvent::Status("startup_services_ready".to_string()));
    }

    pub fn restart_services(&self) {
        log::info!("[Runtime] Restarting agent services...");
        
        // 1. Signal old server to stop
        let old_shutdown = {
            let mut lock = self.agent_shutdown.lock().unwrap();
            lock.take()
        };
        
        if let Some(s) = old_shutdown {
            log::debug!("[Runtime] Sending shutdown signal to old agent...");
            s.store(true, Ordering::Relaxed);
            // Give it a moment to release the port
            std::thread::sleep(std::time::Duration::from_millis(600));
        }

        // 2. Start new services with fresh config
        self.start_services();
    }

    pub fn refresh_ai_services(&self) {
        log::info!("[Runtime] Refreshing AI services due to config change...");
        self.spawn_semantic_initializer();
    }

    // --- Task Spawners (Migrated from app.rs) ---

    pub fn spawn_load_devices(&self) {
        let api_key = self.config.lock().unwrap().api_key.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = TailscaleClient::new(api_key).and_then(|c| c.fetch_devices());
            match result {
                Ok(d)  => { let _ = tx.send(AppEvent::DevicesLoaded(d)); }
                Err(e) => { let _ = tx.send(AppEvent::DevicesFailed(e.to_string())); }
            }
        });
    }

    pub fn spawn_ping(&self, ip: String, manual: bool) {
        let (port, api_key) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone())
        };
        let tx = self.event_tx.clone();
        let ip_addr = ip.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.ping()) {
                Ok(response)  => { let _ = tx.send(AppEvent::AgentPingOk { ip: ip_addr, response, manual }); }
                Err(e) => { let _ = tx.send(AppEvent::AgentPingFailed { ip: ip_addr, error: e.to_string(), manual }); }
            }
        });
    }

    pub fn spawn_ping_device(&self, ip: String, manual: bool) {
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            #[cfg(target_os = "windows")]
            let output = Command::new("ping")
                .args(["-n", "1", "-w", "1200", &ip])
                .output();

            #[cfg(not(target_os = "windows"))]
            let output = Command::new("ping")
                .args(["-c", "1", "-W", "1", &ip])
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let _ = tx.send(AppEvent::Status(format!("device_ping_ok:{}:{}", ip, manual)));
                }
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let msg = if stderr.trim().is_empty() {
                        "host unreachable or timeout".to_string()
                    } else {
                        stderr.trim().to_string()
                    };
                    let _ = tx.send(AppEvent::Status(format!("device_ping_fail:{}:{}:{}", ip, manual, msg)));
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Status(format!("device_ping_fail:{}:{}:{}", ip, manual, e)));
                }
            }
        });
    }

    pub fn spawn_list_remote_files(&self, ip: String) {
        let (port, api_key) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone())
        };
        let tx   = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.list_files()) {
                Ok(f)  => { let _ = tx.send(AppEvent::RemoteFilesLoaded(f)); }
                Err(e) => { let _ = tx.send(AppEvent::RemoteFilesFailed(e.to_string())); }
            }
        });
    }

    pub fn spawn_send_file(&self, ip: String, path: PathBuf, queue_idx: usize) {
        let (port, api_key) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone())
        };
        let tx     = self.event_tx.clone();
        std::thread::spawn(move || {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "unnamed".to_string());
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.upload_file(&path)) {
                Ok(_)  => { let _ = tx.send(AppEvent::FileSent { queue_idx, name }); }
                Err(e) => { let _ = tx.send(AppEvent::FileSendFailed { queue_idx, error: e.to_string() }); }
            }
        });
    }

    pub fn spawn_download_file(&self, ip: String, filename: String) {
        let (port, api_key, dest_dir) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone(), cfg.effective_transfers_dir())
        };
        let tx       = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.download_file(&filename, &dest_dir)) {
                Ok(p)  => { let _ = tx.send(AppEvent::FileDownloaded { name: filename, path: p }); }
                Err(e) => { let _ = tx.send(AppEvent::FileDownloadFailed { name: filename, error: e.to_string() }); }
            }
        });
    }

    pub fn spawn_browse_remote_directory(&self, ip: String, device_id: String, path: PathBuf) {
        let (port, api_key) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone())
        };
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.browse_directory(&path)) {
                Ok(files) => { let _ = tx.send(AppEvent::RemoteBrowseLoaded { device_id, path, files }); }
                Err(e) => { let _ = tx.send(AppEvent::RemoteBrowseFailed { device_id, error: e.to_string() }); }
            }
        });
    }

    pub fn spawn_index_directory(&self, path: PathBuf, device_id: String, device_name: String) {
        let db = Arc::clone(&self.db);
        let config = Arc::clone(&self.config);
        let tx = self.event_tx.clone();
        
        let path_for_thread = path.clone();

        std::thread::spawn(move || {
            let mut path = path_for_thread;
            // Ensure Windows drive letters (e.g., "C:") have a trailing slash for proper walking
            if path.to_string_lossy().len() == 2 && path.to_string_lossy().ends_with(':') {
                path = PathBuf::from(format!("{}\\", path.to_string_lossy()));
            }

            if !path.exists() {
                let _ = tx.send(AppEvent::Status(format!("Path does not exist: {:?}", path)));
                return;
            }

            log::info!("[Runtime] Starting full index task for {:?}", path);
            let _ = tx.send(AppEvent::Status(format!("Indexing {}...", path.display())));

            // Enqueue the root in the persistent queue
            {
                match db.lock() {
                    Ok(guard) => {
                        if let Err(e) = guard.enqueue_index_root(&path) {
                            log::error!("Failed to enqueue index root: {}", e);
                            return;
                        }
                    }
                    Err(_) => { log::error!("DB LOCK FAILED"); return; }
                }
            }

            // Immediately start processing the queue in this thread
            Self::do_process_index_queue(
                db, 
                config,
                tx, 
                device_id, 
                device_name, 
                0,
                Some(path.to_string_lossy().to_string())
            );
        });
    }

    pub fn spawn_index_directories(&self, paths: Vec<PathBuf>, device_id: String, device_name: String) {
        let db = Arc::clone(&self.db);
        let config = Arc::clone(&self.config);
        let tx = self.event_tx.clone();

        std::thread::spawn(move || {
            if paths.is_empty() {
                return;
            }

            let mut accepted_roots = 0u64;

            for mut root in paths {
                if root.to_string_lossy().len() == 2 && root.to_string_lossy().ends_with(':') {
                    root = PathBuf::from(format!("{}\\", root.to_string_lossy()));
                }

                if !root.exists() || should_skip_path(&root) {
                    continue;
                }

                if let Ok(guard) = db.lock() {
                    if guard.enqueue_index_root(&root).is_ok() {
                        accepted_roots += 1;
                        let _ = tx.send(AppEvent::Status(format!(
                            "grid_scan_drive_start:{}|{}",
                            device_id,
                            root.to_string_lossy()
                        )));
                    }
                }
            }

            if accepted_roots == 0 {
                let _ = tx.send(AppEvent::Status("No valid watch roots available for indexing".into()));
                return;
            }

            let _ = tx.send(AppEvent::Status(format!(
                "Indexing across {} root(s)...",
                accepted_roots
            )));

            Self::do_process_index_queue(
                db,
                config,
                tx,
                device_id,
                device_name,
                0,
                None,
            );
        });
    }

    /// Process the persistent index queue until empty.
    /// This is a static-like method to avoid lifetime issues when calling from a thread.
    fn do_process_index_queue(
        db:           Arc<Mutex<Database>>,
        config:       Arc<Mutex<Config>>,
        tx:           mpsc::Sender<AppEvent>,
        device_id:    String,
        device_name:  String,
        total_hint:   u64,
        root_filter:  Option<String>,
    ) {
        let start = std::time::Instant::now();
        let scanned_total = Arc::new(AtomicU64::new(0));
        let dirs_processed = Arc::new(AtomicU64::new(0));
        let max_total_seen = Arc::new(AtomicU64::new(total_hint));
        let mut worker_count = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        worker_count = worker_count.clamp(2, 8);
        if root_filter.is_some() {
            worker_count = worker_count.min(4);
        }

        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let db = Arc::clone(&db);
            let config = Arc::clone(&config);
            let tx = tx.clone();
            let device_id = device_id.clone();
            let device_name = device_name.clone();
            let root_filter = root_filter.clone();
            let scanned_total = Arc::clone(&scanned_total);
            let dirs_processed = Arc::clone(&dirs_processed);
            let max_total_seen = Arc::clone(&max_total_seen);

            workers.push(std::thread::spawn(move || -> u64 {
                let mut local_scanned = 0u64;
                loop {
                    let task = {
                        let guard = match db.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        guard
                            .claim_next_index_task_for_root(root_filter.as_deref())
                            .unwrap_or(None)
                    };

                    let Some((root, dir_str)) = task else {
                        break;
                    };

                    let dir_path = PathBuf::from(&dir_str);
                    let entries = match std::fs::read_dir(&dir_path) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    let cfg_snapshot = config.lock().ok().map(|c| c.clone());
                    let mut subdirs: Vec<PathBuf> = Vec::new();
                    let mut files_to_index: Vec<(PathBuf, u64, Option<i64>, Option<String>)> = Vec::new();

                    for entry in entries.filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if should_skip_path(&path) {
                            continue;
                        }
                        let meta = match entry.metadata() { Ok(m) => m, Err(_) => continue };

                        if meta.is_dir() {
                            let name = path.file_name().unwrap_or_default().to_string_lossy();
                            if should_skip_dir(&name) {
                                continue;
                            }
                            subdirs.push(path);
                        } else if meta.is_file() {
                            let size = meta.len();
                            let modified = meta.modified().ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs() as i64);
                            let quick_hash = thegrid_core::quick_hash_file(&path).ok();
                            files_to_index.push((path, size, modified, quick_hash));
                        }
                    }

                    let mut dir_count = 0u64;
                    let db_guard = match db.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };

                    for (path, size, modified, quick_hash) in &files_to_index {
                        if let Ok(fid) = db_guard.index_file_with_source(
                            &device_id,
                            &device_name,
                            path,
                            *size,
                            *modified,
                            quick_hash.as_deref(),
                            None,
                            thegrid_core::DetectionSource::FullScan,
                            chrono::Utc::now().timestamp(),
                        ) {
                            if let Some(cfg) = cfg_snapshot.as_ref() {
                                apply_automation_rules_for_file(
                                    &db_guard,
                                    cfg,
                                    fid,
                                    path,
                                    *size,
                                    *modified,
                                );
                            }
                            dir_count += 1;
                        }
                    }

                    for s in subdirs {
                        let _ = db_guard.conn.execute(
                            "INSERT OR IGNORE INTO index_queue (root_path, dir_path) VALUES (?, ?)",
                            params![root.as_str(), s.to_string_lossy()]
                        );
                    }

                    local_scanned += dir_count;
                    let global = scanned_total.fetch_add(dir_count, Ordering::Relaxed) + dir_count;
                    let processed = dirs_processed.fetch_add(1, Ordering::Relaxed) + 1;
                    let pending_dirs = db_guard
                        .pending_index_task_count_for_root(root_filter.as_deref())
                        .unwrap_or(0);

                    let avg_files_per_dir = (global as f64 / processed as f64).max(1.0);
                    let est_remaining = (pending_dirs as f64 * avg_files_per_dir).round() as u64;
                    let dynamic_total = global.saturating_add(est_remaining).max(global);

                    let progress_scanned = if total_hint > 0 {
                        global.min(total_hint)
                    } else {
                        global
                    };

                    let progress_total = if total_hint > 0 {
                        total_hint.max(progress_scanned)
                    } else {
                        dynamic_total
                    };

                    let mut observed_max = max_total_seen.load(Ordering::Relaxed);
                    while progress_total > observed_max {
                        match max_total_seen.compare_exchange_weak(
                            observed_max,
                            progress_total,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(actual) => observed_max = actual,
                        }
                    }
                    let stable_total = max_total_seen.load(Ordering::Relaxed).max(progress_scanned);

                    let _ = tx.send(AppEvent::IndexProgress {
                        scanned: progress_scanned,
                        total:   stable_total,
                        current: dir_path.file_name().unwrap_or_default().to_string_lossy().into(),
                        ext:     None,
                        estimated_total: total_hint == 0,
                    });
                    let _ = tx.send(AppEvent::Status(format!(
                        "grid_scan_progress:{}|{}|{}|{}|{}|{}",
                        device_id,
                        root,
                        dir_path.to_string_lossy(),
                        progress_scanned,
                        stable_total,
                        pending_dirs
                    )));
                }
                local_scanned
            }));
        }

        let mut scanned_count = 0u64;
        for worker in workers {
            if let Ok(n) = worker.join() {
                scanned_count += n;
            }
        }

        let _ = tx.send(AppEvent::IndexComplete {
            device_id,
            files_added: scanned_count,
            duration_ms: start.elapsed().as_millis() as u64,
        });
        let _ = tx.send(AppEvent::Status(format!(
            "grid_scan_complete:{}|{}|{}",
            device_name,
            scanned_count,
            start.elapsed().as_millis() as u64
        )));

        // Background worker triggers could go here
    }

    /// Scan the local index for exact-duplicate files (same hash + size).
    /// Emits `AppEvent::DuplicatesFound` with groups ready for UI or CLI display.
    pub fn spawn_duplicates_scan(&self) {
        self.spawn_duplicates_scan_filtered(thegrid_core::models::DuplicateScanFilter::default());
    }

    /// Scan duplicates with GUI-provided filters to keep large indexes manageable.
    pub fn spawn_duplicates_scan_filtered(&self, filter: thegrid_core::models::DuplicateScanFilter) {
        let db_path = self.db_path.clone();
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            log::info!("[Runtime] Starting duplicate file scan...");

            // Open a *dedicated* read-only connection so this long-running
            // query never contends with the shared Arc<Mutex<Database>> used
            // by the watcher, media analyzer, and embed worker.
            let scan_db = match Database::open(&db_path) {
                Ok(d)  => d,
                Err(e) => { log::error!("[Runtime] Duplicate scan: DB open failed: {}", e); return; }
            };
            let groups = match scan_db.get_duplicate_groups() {
                Ok(g)  => g,
                Err(e) => { log::error!("[Runtime] Duplicate scan DB error: {}", e); return; }
            };
            // scan_db dropped here — connection closed.

            let raw_group_count = groups.len();
            let ext_filter: HashSet<String> = filter
                .include_extensions
                .iter()
                .map(|e| e.trim().trim_start_matches('.').to_lowercase())
                .filter(|e| !e.is_empty())
                .collect();
            let path_prefix = filter.path_prefix
                .as_deref()
                .map(|p| p.to_lowercase())
                .filter(|p| !p.is_empty());

            let mut filtered_groups: Vec<(String, u64, Vec<thegrid_core::models::FileSearchResult>)> = Vec::new();
            for (hash, size, files) in groups {
                if size < filter.min_size_bytes {
                    continue;
                }
                let mut keep = Vec::new();
                for file in files {
                    if filter.exclude_system_paths && thegrid_core::should_skip_path(&file.path) {
                        continue;
                    }
                    if let Some(device_id) = filter.device_id.as_deref() {
                        if !device_id.is_empty() && file.device_id != device_id {
                            continue;
                        }
                    }
                    if !ext_filter.is_empty() {
                        let file_ext = file.ext.clone().unwrap_or_default().to_lowercase();
                        if !ext_filter.contains(&file_ext) {
                            continue;
                        }
                    }
                    if let Some(prefix) = path_prefix.as_deref() {
                        let fp = file.path.to_string_lossy().to_lowercase();
                        if !fp.starts_with(prefix) {
                            continue;
                        }
                    }
                    keep.push(file);
                }

                if keep.len() > 1 {
                    filtered_groups.push((hash, size, keep));
                }
            }

            filtered_groups.sort_by(|a, b| {
                let aw = a.1.saturating_mul(a.2.len().saturating_sub(1) as u64);
                let bw = b.1.saturating_mul(b.2.len().saturating_sub(1) as u64);
                bw.cmp(&aw)
            });

            let max_groups = filter.max_groups.max(1);
            if filtered_groups.len() > max_groups {
                filtered_groups.truncate(max_groups);
            }

            log::info!(
                "[Runtime] Duplicate scan found {} raw group(s), {} filtered group(s)",
                raw_group_count,
                filtered_groups.len()
            );
            let _ = tx.send(AppEvent::DuplicatesFound(filtered_groups));
        });
    }

    pub fn spawn_idle_work(&self) {
        log::info!("Idle worker triggered — looking for unfinished tasks");
        let db = Arc::clone(&self.db);
        let config = Arc::clone(&self.config);
        let tx = self.event_tx.clone();

        let device_id   = { self.config.lock().unwrap().device_name.clone() };
        let device_name = device_id.clone();
        
        std::thread::spawn(move || {
            Self::do_process_index_queue(
                db, 
                config,
                tx, 
                device_id, 
                device_name, 
                0,
                None
            );
        });
    }


    pub fn spawn_incremental_index(&self, changes: Vec<FileChange>) {
        let db = Arc::clone(&self.db);
        let config = Arc::clone(&self.config);
        let (device_id, device_name) = {
            let cfg = self.config.lock().unwrap();
            (cfg.device_name.clone(), cfg.device_name.clone())
        };
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = db.lock().map(|guard| {
                guard.index_changed_paths(&device_id, &device_name, &changes)
            });
            match result {
                Ok(Ok((updated, deleted, renamed))) => {
                    let cfg = config.lock().ok().map(|c| c.clone());
                    if let (Some(cfg), Ok(guard)) = (cfg, db.lock()) {
                        for change in &changes {
                            let candidate = match change.kind {
                                thegrid_core::FileChangeKind::Created | thegrid_core::FileChangeKind::Modified => Some(change.path.clone()),
                                thegrid_core::FileChangeKind::Renamed => change.new_path.clone(),
                                thegrid_core::FileChangeKind::Deleted => None,
                            };

                            if let Some(path) = candidate {
                                if !path.exists() || !path.is_file() {
                                    continue;
                                }
                                let fp = change.fingerprint.clone()
                                    .or_else(|| fingerprint_file(&path).ok());
                                if let Some(fp) = fp {
                                    if let Ok(Some(fid)) = guard.get_file_id_by_path(&device_id, &path) {
                                        apply_automation_rules_for_file(
                                            &guard,
                                            &cfg,
                                            fid,
                                            &path,
                                            fp.size,
                                            fp.modified,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    let _ = tx.send(AppEvent::IndexUpdated { paths_updated: updated + deleted + renamed });
                }
                _ => {}
            }
        });
    }

    pub fn spawn_sync_node(&self, device_id: String, ip: String, hostname: String) {
        let db          = self.db.clone();
        let event_tx    = self.event_tx.clone();
        let sync_health = self.sync_health.clone();
        let (api_key, port, requester_device) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port, cfg.device_name.clone())
        };

        std::thread::spawn(move || {
            let _ = event_tx.send(AppEvent::Status(format!(
                "grid_sync_start:{}|{}",
                device_id,
                hostname
            )));
            let last_ts = match db.lock() {
                Ok(guard) => guard.get_node_sync_ts(&device_id).unwrap_or(0),
                Err(_)    => 0,
            };

            let client = match AgentClient::new(&ip, port, api_key) {
                Ok(c) => c,
                Err(e) => {
                    let now = chrono::Utc::now().timestamp();
                    if let Ok(mut guard) = sync_health.lock() {
                        let metrics = guard.entry(device_id.clone()).or_default();
                        metrics.mark_sync_failure(now);
                        let _ = event_tx.send(AppEvent::SyncHealthUpdated {
                            device_id: device_id.clone(),
                            metrics: metrics.clone(),
                        });
                    }
                    let _ = event_tx.send(AppEvent::SyncFailed { device_id, error: e.to_string() });
                    return;
                }
            };

            let delta = match client.sync_index(last_ts, Some(&requester_device)) {
                Ok(r) => r,
                Err(e) => {
                    let now = chrono::Utc::now().timestamp();
                    if let Ok(mut guard) = sync_health.lock() {
                        let metrics = guard.entry(device_id.clone()).or_default();
                        metrics.mark_sync_failure(now);
                        let _ = event_tx.send(AppEvent::SyncHealthUpdated {
                            device_id: device_id.clone(),
                            metrics: metrics.clone(),
                        });
                    }
                    let _ = event_tx.send(AppEvent::SyncFailed { device_id, error: e.to_string() });
                    return;
                }
            };

            let mut count = 0;
            let mut max_ts = last_ts;
            let now = chrono::Utc::now().timestamp();
            let tombstone_count = delta.tombstones.len() as u64;
            let mut detection_sources = DetectionSourceDistribution::default();
            if let Ok(guard) = db.lock() {
                for r in delta.files {
                    let mod_ts = r.modified.unwrap_or(0);
                    if r.indexed_at > max_ts { max_ts = r.indexed_at; }
                    if mod_ts > max_ts { max_ts = mod_ts; }
                    detection_sources.increment(r.detected_by);
                    if guard.upsert_remote_file(r).is_ok() {
                        count += 1;
                    }
                }
                for tombstone in delta.tombstones {
                    if tombstone.deleted_at > max_ts {
                        max_ts = tombstone.deleted_at;
                    }
                    detection_sources.increment(tombstone.detected_by);
                    let _ = guard.apply_remote_tombstone(&tombstone);
                }
                let _ = guard.update_node_sync_ts(&device_id, &hostname, max_ts);
            }

            if let Ok(mut guard) = sync_health.lock() {
                let metrics = guard.entry(device_id.clone()).or_default();
                metrics.mark_sync_success(now, tombstone_count, detection_sources);
                let _ = event_tx.send(AppEvent::SyncHealthUpdated {
                    device_id: device_id.clone(),
                    metrics: metrics.clone(),
                });
            }

            if let Ok(guard) = db.lock() {
                if let Ok((groups, files, bytes, known_devices)) = guard.crosscheck_duplicates_for_device(&device_id) {
                    let _ = event_tx.send(AppEvent::Status(format!(
                        "crosscheck:{}|{}|{}|{}|{}",
                        device_id,
                        groups,
                        files,
                        bytes,
                        known_devices
                    )));
                }
            }

            let _ = event_tx.send(AppEvent::SyncComplete { device_id, files_added: count });
        });
    }

    pub fn spawn_semantic_initializer(&self) {
        let event_tx = self.event_tx.clone();
        let db       = self.db.clone();
        let semantic = self.semantic_search.clone();
        let config   = self.config.clone();
        let ui_priority = Arc::clone(&self.ui_priority_mode);

        std::thread::spawn(move || {
            const LOCAL_OLLAMA: &str = "http://127.0.0.1:11434";
            const REMOTE_LOCALAI: &str = "http://100.67.58.127:8080";
            let prefer_remote_first = !thegrid_ai::AiNodeDetector::new().is_ai_node();

            // Resolve provider URL + model — explicit config wins, then auto-detect with fallback.
            let (resolved_url, resolved_model) = {
                let cfg = config.lock().unwrap();
                match (cfg.ai_provider_url.clone(), cfg.ai_model.clone()) {
                    (Some(u), m) => (u, m.unwrap_or_else(|| "nomic-embed-text".to_string())),
                    (None, _) => {
                        // Auto-detect: try local Ollama first, then remote LocalAI, then fallback
                        let endpoints = if prefer_remote_first {
                            vec![
                                (REMOTE_LOCALAI, "Tablet LocalAI (preferred for non-AI node)"),
                                (LOCAL_OLLAMA, "Local Ollama (fallback)"),
                            ]
                        } else {
                            vec![
                                (LOCAL_OLLAMA, "Local Ollama (GPU-accel)"),
                                (REMOTE_LOCALAI, "Tablet LocalAI (backup)"),
                            ]
                        };

                        let mut found_endpoint = None;
                        for (url, label) in endpoints {
                            match thegrid_ai::probe_ollama_models(url) {
                                Some(models) if !models.is_empty() => {
                                    let best = if url == REMOTE_LOCALAI {
                                        thegrid_ai::pick_tablet_embed_model(&models)
                                            .or_else(|| thegrid_ai::pick_best_embed_model(&models))
                                            .unwrap_or_else(|| "all-minilm".to_string())
                                    } else {
                                        thegrid_ai::pick_best_embed_model(&models)
                                            .unwrap_or_else(|| "nomic-embed-text".to_string())
                                    };
                                    log::info!("[AI] {} → {} model(s), selecting '{}'", label, models.len(), best);
                                    if url == REMOTE_LOCALAI {
                                        let _ = event_tx.send(AppEvent::Status(format!(
                                            "tablet_model_selected:{}",
                                            best
                                        )));
                                    }
                                    found_endpoint = Some((url.to_string(), best));
                                    break;
                                }
                                _ => {
                                    // Try next endpoint
                                    log::debug!("[AI] {} not reachable, trying next...", label);
                                    continue;
                                }
                            }
                        }

                        // Also try LocalAI via /v1/models endpoint
                        if found_endpoint.is_none() {
                            if let Some(models) = thegrid_ai::probe_localai_models(REMOTE_LOCALAI) {
                                if !models.is_empty() {
                                    let best = models.first().cloned().unwrap_or_else(|| "deepseek-coder-16b.gguf".to_string());
                                    log::info!("[AI] Tablet LocalAI (OpenAI API) → {} model(s), selecting '{}'", models.len(), best);
                                    found_endpoint = Some((REMOTE_LOCALAI.to_string(), best));
                                }
                            }
                        }

                        match found_endpoint {
                            Some((url, model)) => (url, model),
                            None => {
                                log::warn!("[AI] No remote AI provider found, falling back to local deterministic embeddings.");
                                ("local://fastembed".to_string(), "local-hash-embed-v1".to_string())
                            }
                        }
                    }
                }
            };

            // Persist discovered config for subsequent runs.
            if let Ok(mut cfg) = config.lock() {
                if cfg.ai_provider_url.is_none() {
                    cfg.ai_provider_url = Some(resolved_url.clone());
                    cfg.ai_model = Some(resolved_model.clone());
                    let _ = cfg.save();
                }
            }

            log::info!("[AI] Initializing embedding provider: model={} url={}", resolved_model, resolved_url);
            let provider_result: Result<Arc<dyn EmbeddingProvider>, anyhow::Error> =
                if resolved_url.starts_with("local://") {
                    thegrid_ai::FastEmbedProvider::new().map(|p| Arc::new(p) as Arc<dyn EmbeddingProvider>)
                } else {
                    match thegrid_ai::HttpEmbeddingProvider::new(resolved_model.clone(), resolved_url.clone()) {
                        Ok(p) => Ok(Arc::new(p) as Arc<dyn EmbeddingProvider>),
                        Err(e) => {
                            log::warn!("[AI] Remote provider init failed ({}), falling back to local deterministic embeddings", e);
                            thegrid_ai::FastEmbedProvider::new().map(|p| Arc::new(p) as Arc<dyn EmbeddingProvider>)
                        }
                    }
                };

            match provider_result {
                Ok(p) => {
                    match SemanticSearch::new(p) {
                        Ok(search) => {
                            let model_id = search.model_id().to_string();
                            {
                                let mut lock = semantic.lock().expect("semantic lock");
                                *lock = Some(search);
                            }
                            log::info!("[AI] Semantic engine ready: model={} vectors=0", model_id);
                            let _ = event_tx.send(AppEvent::SemanticReady);

                            // Defer heavy vector warmup while interactive screens are prioritized.
                            let db_warm = Arc::clone(&db);
                            let semantic_warm = Arc::clone(&semantic);
                            let ui_priority_warm = Arc::clone(&ui_priority);
                            let event_tx_warm = event_tx.clone();
                            std::thread::spawn(move || {
                                while ui_priority_warm.load(Ordering::Relaxed) {
                                    std::thread::sleep(std::time::Duration::from_millis(800));
                                }

                                let all = match db_warm.lock() {
                                    Ok(guard) => guard.get_all_embeddings().unwrap_or_default(),
                                    Err(_) => Vec::new(),
                                };
                                if all.is_empty() {
                                    return;
                                }

                                let mut loaded = 0usize;
                                if let Ok(mut lock) = semantic_warm.lock() {
                                    if let Some(search) = lock.as_mut() {
                                        for (fid, vec) in all {
                                            let _ = search.add_vector(fid, &vec);
                                            loaded += 1;
                                            if loaded % 3000 == 0 {
                                                std::thread::sleep(std::time::Duration::from_millis(20));
                                            }
                                        }
                                    }
                                }

                                log::info!("[AI] Warmed semantic index with {} vectors", loaded);
                                let _ = event_tx_warm.send(AppEvent::Status(format!("semantic_warmup_loaded:{}", loaded)));
                            });
                        }
                        Err(e) => {
                            let _ = event_tx.send(AppEvent::SemanticFailed(format!("Vector engine: {}", e)));
                        }
                    }
                }
                Err(e) => {
                    log::error!("[AI] Provider init failed: {}", e);
                    let _ = event_tx.send(AppEvent::SemanticFailed(format!("Provider: {}", e)));
                }
            }
        });
    }

    pub fn spawn_embedding_worker(&self) {
        if self.embedding_worker_running.swap(true, Ordering::SeqCst) {
            return;
        }

        let db       = self.db.clone();
        let event_tx = self.event_tx.clone();
        let semantic = self.semantic_search.clone();
        let ui_priority = Arc::clone(&self.ui_priority_mode);
        let running = Arc::clone(&self.embedding_worker_running);
        let remote_ai = self.remote_ai_nodes.clone();
        let termux_agent = self.termux_agent.clone();
        let tailscale_peers = Arc::clone(&self.tailscale_peers);
        let compute_router = Arc::clone(&self.compute_router);
        let (api_key, port, tablet_assist_enabled, tablet_cpu_max, tablet_gpu_max, max_parallel_requests) = {
            let cfg = self.config.lock().unwrap();
            (
                cfg.api_key.clone(),
                cfg.agent_port,
                cfg.ai_tablet_assist,
                cfg.ai_tablet_assist_cpu_max_pct,
                cfg.ai_tablet_assist_gpu_max_pct,
                cfg.embedding_parallel_requests.max(1),
            )
        };
        
        std::thread::spawn(move || {
            let total = db.lock().ok().and_then(|g| g.count_unindexed_files().ok()).unwrap_or(0);
            let mut indexed = 0;
            let mut last_termux_heartbeat = std::time::Instant::now() - std::time::Duration::from_secs(60);
            let mut last_tablet_probe = std::time::Instant::now() - std::time::Duration::from_secs(60);
            let mut tablet_is_idle = false;
            // Cache the best router-resolved peer so we don't re-probe every batch.
            let mut router_peer_cache: Option<(String, u16)> = None;
            let mut last_router_probe = std::time::Instant::now() - std::time::Duration::from_secs(300);

            loop {
                let ui_mode = ui_priority.load(Ordering::Relaxed);
                let local_engine = {
                    let lock = semantic.lock().expect("semantic lock");
                    lock.as_ref().map(|s| (s.provider(), s.model_id().to_string()))
                };
                let local_provider = local_engine.as_ref().map(|(p, _)| Arc::clone(p));
                let local_model_id = local_engine
                    .as_ref()
                    .map(|(_, m)| m.clone())
                    .unwrap_or_else(|| "remote".to_string());

                // Lightweight heartbeat + auto-degradation: if current Termux endpoint fails,
                // attempt to re-establish via OTG -> LAN -> Tailscale.
                if last_termux_heartbeat.elapsed() >= std::time::Duration::from_secs(15) {
                    let mut heartbeat_status: Option<(String, String)> = None;
                    let mut termux_probe_target: Option<(String, u16)> = None;
                    {
                        let mut lock = termux_agent.lock().unwrap();
                        if let Some(agent) = lock.as_mut() {
                            let endpoint = agent.endpoint().to_string();
                            if let Some((ip, p)) = AppRuntime::parse_agent_target(&endpoint, port) {
                                termux_probe_target = Some((ip.clone(), p));
                                let ok = AgentClient::new(&ip, p, api_key.clone())
                                    .and_then(|c| c.ping())
                                    .map(|r| r.authorized)
                                    .unwrap_or(false);

                                if !ok {
                                    match agent.establish_best_connection() {
                                        Ok(()) => {
                                            heartbeat_status = Some((
                                                "termux_recovered".to_string(),
                                                format!("{}:{}", agent.connection_method(), agent.endpoint()),
                                            ));
                                        }
                                        Err(e) => {
                                            heartbeat_status = Some((
                                                "termux_unreachable".to_string(),
                                                e.to_string(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if tablet_assist_enabled
                        && last_tablet_probe.elapsed() >= std::time::Duration::from_secs(20)
                    {
                        let new_idle = termux_probe_target
                            .and_then(|(ip, p)| {
                                AgentClient::new(&ip, p, api_key.clone())
                                    .ok()
                                    .and_then(|c| c.get_telemetry().ok())
                            })
                            .map(|t| {
                                let ai_state = t.ai_status
                                    .unwrap_or_else(|| "idle".to_string())
                                    .to_ascii_lowercase();
                                let ai_ok = ai_state.contains("idle") || ai_state.contains("ready");
                                let cpu_ok = t.cpu_pct <= tablet_cpu_max;
                                let gpu_ok = t.gpu_pct.unwrap_or(0.0) <= tablet_gpu_max;
                                ai_ok && cpu_ok && gpu_ok
                            })
                            .unwrap_or(false);

                        if new_idle != tablet_is_idle {
                            let mode = if new_idle { "idle" } else { "busy" };
                            let _ = event_tx.send(AppEvent::Status(format!(
                                "tablet_assist:{}:cpu<={:.0}% gpu<={:.0}%",
                                mode,
                                tablet_cpu_max,
                                tablet_gpu_max
                            )));
                        }
                        tablet_is_idle = new_idle;
                        last_tablet_probe = std::time::Instant::now();
                    }

                    if let Some((kind, payload)) = heartbeat_status {
                        let _ = event_tx.send(AppEvent::Status(format!("{}:{}", kind, payload)));
                    }
                    last_termux_heartbeat = std::time::Instant::now();
                }

                // Pull from priority queue first; fall back to unqueued files.
                let batch: Vec<(i64, String)> = match db.lock() {
                    Ok(guard) => {
                        let effective_parallel = if ui_mode { 1 } else { max_parallel_requests };
                        let target_batch = (20usize).saturating_mul(effective_parallel).min(200);
                        let queued = guard.get_embedding_queue_batch(target_batch).unwrap_or_default();
                        if queued.is_empty() {
                            // Queue is drained; re-seed from files without embeddings
                            let unqueued = guard.get_files_needing_embedding(target_batch).unwrap_or_default();
                            for (fid, _text) in &unqueued {
                                let _ = guard.queue_embedding_for_file(*fid, 3);
                            }
                            unqueued
                        } else {
                            queued.into_iter().map(|(fid, text, _priority)| (fid, text)).collect()
                        }
                    }
                    Err(_) => break,
                };
                
                if batch.is_empty() { 
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }

                let termux_target = {
                    let lock = termux_agent.lock().unwrap();
                    lock.as_ref().and_then(|a| AppRuntime::parse_agent_target(a.endpoint(), port))
                };

                let remote_node = {
                    let nodes = remote_ai.lock().unwrap();
                    nodes.values().next().cloned()
                };

                let work_items: Vec<(usize, i64, String)> = batch
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (fid, text))| (idx, fid, text))
                    .collect();

                // Refresh the best compute-router peer every 3 minutes.
                // route_task probes /v1/compute/status (2s timeout) so we cache the result.
                if last_router_probe.elapsed() >= std::time::Duration::from_secs(180) {
                    let peers = tailscale_peers.lock().unwrap().clone();
                    router_peer_cache = match compute_router.route_task(
                        &ComputeTaskType::TextEmbedding,
                        &peers,
                    ) {
                        crate::compute_router::RouteTarget::Remote { ip, .. } => {
                            log::info!("[EmbedWorker] Compute router selected peer: {}", ip);
                            Some((ip, port))
                        }
                        crate::compute_router::RouteTarget::Local => {
                            log::debug!("[EmbedWorker] Compute router: no capable remote peer, using local only");
                            None
                        }
                    };
                    last_router_probe = std::time::Instant::now();
                }
                let router_peer = router_peer_cache.clone();

                let effective_parallel = if ui_mode { 1 } else { max_parallel_requests };
                let worker_count = effective_parallel
                    .max(1)
                    .min(8)
                    .min(work_items.len().max(1));
                let chunk_size = ((work_items.len() + worker_count - 1) / worker_count).max(1);
                let mut handles = Vec::new();

                for chunk in work_items.chunks(chunk_size) {
                    let items = chunk.to_vec();
                    let key = api_key.clone();
                    let lp = local_provider.clone();
                    let model_id = local_model_id.clone();
                    let tt = termux_target.clone();
                    let rn = remote_node.clone();
                    let rp = router_peer.clone();
                    let use_tablet_assist = tablet_assist_enabled;
                    let tablet_idle_now = tablet_is_idle;

                    let handle = std::thread::spawn(move || {
                        let mut out: Vec<(usize, i64, Vec<f32>, String)> = Vec::with_capacity(items.len());
                        for (idx, fid, text) in items {
                            let tablet_first = use_tablet_assist
                                && tablet_idle_now
                                && tt.is_some()
                                && (lp.is_none() || idx % 2 == 1);

                            let mut vector: Vec<f32> = Vec::new();
                            let mut source = "remote".to_string();

                            if tablet_first {
                                if let Some((ip, p)) = &tt {
                                    if let Ok(client) = AgentClient::new(ip, *p, key.clone()) {
                                        if let Ok(v) = client.remote_embed(&text) {
                                            vector = v;
                                        }
                                    }
                                }
                                if vector.is_empty() {
                                    if let Some(provider) = &lp {
                                        if let Ok(v) = provider.embed(&text) {
                                            vector = v;
                                            source = model_id.clone();
                                        }
                                    }
                                }
                            } else {
                                if let Some(provider) = &lp {
                                    if let Ok(v) = provider.embed(&text) {
                                        vector = v;
                                        source = model_id.clone();
                                    }
                                }
                                if vector.is_empty() {
                                    if let Some((ip, p)) = &tt {
                                        if let Ok(client) = AgentClient::new(ip, *p, key.clone()) {
                                            if let Ok(v) = client.remote_embed(&text) {
                                                vector = v;
                                            }
                                        }
                                    }
                                }
                            }

                            if vector.is_empty() {
                                if let Some(ip) = &rn {
                                    if let Ok(client) = AgentClient::new(ip, port, key.clone()) {
                                        if let Ok(v) = client.remote_embed(&text) {
                                            vector = v;
                                        }
                                    }
                                }
                            }

                            // Final fallback: compute-router selected best available Tailscale peer
                            if vector.is_empty() {
                                if let Some((ip, p)) = &rp {
                                    if let Ok(client) = AgentClient::new(ip, *p, key.clone()) {
                                        if let Ok(v) = client.remote_embed(&text) {
                                            vector = v;
                                            source = format!("router:{}", ip);
                                        }
                                    }
                                }
                            }

                            out.push((idx, fid, vector, source));
                        }
                        out
                    });
                    handles.push(handle);
                }

                let mut merged: Vec<(usize, i64, Vec<f32>, String)> = Vec::new();
                for handle in handles {
                    if let Ok(mut rows) = handle.join() {
                        merged.append(&mut rows);
                    }
                }
                merged.sort_by_key(|(idx, _, _, _)| *idx);

                for (_, fid, vector, source) in merged {
                    if !vector.is_empty() {
                        if let Ok(db_lock) = db.lock() {
                            let _ = db_lock.save_embedding(fid, &source, &vector);
                            let _ = db_lock.dequeue_embedding(fid);
                        }

                        let mut lock = semantic.lock().expect("semantic lock");
                        if let Some(search) = &mut *lock {
                            let _ = search.add_vector(fid, &vector);
                        }
                    } else {
                        if let Ok(db_lock) = db.lock() {
                            let _ = db_lock.increment_embedding_attempts(fid);
                        }
                    }

                    indexed += 1;
                    let _ = event_tx.send(AppEvent::EmbeddingProgress { indexed, total });
                }

                std::thread::sleep(std::time::Duration::from_millis(if ui_mode { 350 } else { 100 }));
            }

            running.store(false, Ordering::SeqCst);
        });
    }

    pub fn spawn_hashing_worker(&self) {
        if self.hash_worker_running.swap(true, Ordering::SeqCst) {
            return;
        }

        let db = Arc::clone(&self.db);
        let config = Arc::clone(&self.config);
        let event_tx = self.event_tx.clone();
        let running = Arc::clone(&self.hash_worker_running);
        let ui_priority = Arc::clone(&self.ui_priority_mode);

        std::thread::spawn(move || {
            log::info!("[Runtime] Starting background hashing worker...");
            let mut hashed_this_session = 0usize;
            // total = files already hashed before this session + still-remaining
            // We track remaining so we can compute a stable total even as new files arrive.
            let mut total_to_hash: usize = db.lock()
                .map(|g| g.count_files_needing_hash().unwrap_or(0))
                .unwrap_or(0);
            // hashed_this_session starts at 0; total displayed = hashed_this_session + remaining
            // Emit initial state so the UI can display the bar immediately.
            let _ = event_tx.send(AppEvent::HashingProgress {
                hashed: 0,
                total: total_to_hash,
            });

            loop {
                let ui_mode = ui_priority.load(Ordering::Relaxed);
                let batch_size = if ui_mode { 8 } else { 50 };
                // Re-count remaining each batch to account for newly-indexed files.
                let remaining = db.lock()
                    .map(|g| g.count_files_needing_hash().unwrap_or(0))
                    .unwrap_or(0);
                // Stable total: at least as large as already hashed + still remaining.
                total_to_hash = total_to_hash.max(hashed_this_session + remaining);

                // 1. Get files needing hashing
                let batch = match db.lock() {
                    Ok(guard) => match guard.get_files_needing_hash(batch_size) {
                        Ok(b) => b,
                        Err(e) => {
                            log::error!("[Runtime] Hashing error in DB: {}", e);
                            break;
                        }
                    },
                    Err(_) => break,
                };

                if batch.is_empty() {
                    // Nothing left; emit 100% and wait for new files.
                    let _ = event_tx.send(AppEvent::HashingProgress {
                        hashed: total_to_hash,
                        total:  total_to_hash,
                    });
                    std::thread::sleep(std::time::Duration::from_secs(if ui_mode { 45 } else { 30 }));
                    continue;
                }

                for (fid, path) in batch {
                    if path.exists() && path.is_file() {
                        // Quick-hash pre-filter: if no other file shares (quick_hash, size),
                        // skip the full read and mark as SKIP_UNIQUE to save I/O.
                        let should_skip = path.metadata().ok().and_then(|meta| {
                            let size = meta.len();
                            thegrid_core::quick_hash_file(&path).ok().map(|qh| (qh, size))
                        }).and_then(|(qh, size)| {
                            db.lock().ok().and_then(|guard| {
                                guard.has_quick_hash_peer(fid, &qh, size).ok().map(|has_peer| !has_peer)
                            })
                        }).unwrap_or(false);

                        if should_skip {
                            if let Ok(guard) = db.lock() {
                                let _ = guard.mark_file_skip_unique(fid);
                            }
                            hashed_this_session += 1;
                        } else {
                            match thegrid_core::hash_file(&path) {
                                Ok(h) => {
                                    if let Ok(guard) = db.lock() {
                                        let _ = guard.update_file_hash(fid, &h);
                                        if let Ok(cfg) = config.lock() {
                                            if let Ok(meta) = path.metadata() {
                                                let modified = meta.modified().ok()
                                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                                    .map(|d| d.as_secs() as i64);
                                                apply_automation_rules_for_file(
                                                    &guard,
                                                    &cfg,
                                                    fid,
                                                    &path,
                                                    meta.len(),
                                                    modified,
                                                );
                                            }
                                        }
                                    }
                                    hashed_this_session += 1;
                                }
                                Err(e) => {
                                    log::warn!("[Runtime] Failed to hash {:?}: {}", path, e);
                                    if let Ok(guard) = db.lock() {
                                        let _ = guard.update_file_hash(fid, &format!("ERR_{}", e));
                                    }
                                    hashed_this_session += 1;
                                }
                            }
                        }
                    } else {
                        if let Ok(guard) = db.lock() {
                            let _ = guard.delete_file_by_id(fid);
                        }
                        hashed_this_session += 1;
                    }

                    // Emit progress every 5 files for responsive UI.
                    if hashed_this_session % 5 == 0 {
                        let _ = event_tx.send(AppEvent::HashingProgress {
                            hashed: hashed_this_session.min(total_to_hash),
                            total:  total_to_hash,
                        });
                    }
                }

                // Emit at the end of every batch.
                let _ = event_tx.send(AppEvent::HashingProgress {
                    hashed: hashed_this_session.min(total_to_hash),
                    total:  total_to_hash,
                });

                // Persist progress checkpoint for observability (resume not yet implemented).
                if hashed_this_session % 200 == 0 {
                    if let Ok(guard) = db.lock() {
                        let _ = guard.set_hashing_checkpoint(hashed_this_session as i64);
                    }
                }

                // Yield to other threads
                std::thread::sleep(std::time::Duration::from_millis(if ui_mode { 250 } else { 50 }));
            }

            running.store(false, Ordering::SeqCst);
        });
    }

    pub fn spawn_media_analyzer_worker(&self) {
        if self.media_worker_running.swap(true, Ordering::SeqCst) {
            return;
        }

        let db = Arc::clone(&self.db);
        let cfg = Arc::clone(&self.config);
        let event_tx = self.event_tx.clone();
        let analyzer = Arc::clone(&self.media_analyzer);
        let termux_agent = Arc::clone(&self.termux_agent);
        let ui_priority = Arc::clone(&self.ui_priority_mode);
        let running = Arc::clone(&self.media_worker_running);

        std::thread::spawn(move || {
            log::info!("[Runtime] Starting background media analyzer worker...");
            let mut batch_size: usize;
            let mut analyzed_count = 0;
            let mut last_tablet_probe = std::time::Instant::now() - std::time::Duration::from_secs(60);
            let mut tablet_idle_for_assist = false;
            let mut last_mode_status = std::time::Instant::now() - std::time::Duration::from_secs(15);

            loop {
                let ui_mode = ui_priority.load(Ordering::Relaxed);
                let az = {
                    let lock = analyzer.lock().unwrap();
                    lock.clone()
                };

                if az.is_none() {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    continue;
                }
                let az = az.unwrap();

                let (mode_raw, api_key, agent_port, tablet_assist_enabled, tablet_cpu_max, tablet_gpu_max) = cfg
                    .lock()
                    .map(|c| {
                        (
                            c.media_processing_mode.clone(),
                            c.api_key.clone(),
                            c.agent_port,
                            c.ai_tablet_assist,
                            c.ai_tablet_assist_cpu_max_pct,
                            c.ai_tablet_assist_gpu_max_pct,
                        )
                    })
                    .unwrap_or_else(|_| ("auto".to_string(), String::new(), 5000, true, 55.0, 60.0));
                let mode = AppRuntime::parse_media_processing_mode(&mode_raw);

                if tablet_assist_enabled
                    && last_tablet_probe.elapsed() >= std::time::Duration::from_secs(20)
                {
                    let termux_target = {
                        let lock = termux_agent.lock().unwrap();
                        lock.as_ref().and_then(|a| {
                            AppRuntime::parse_agent_target(a.endpoint(), agent_port)
                        })
                    };

                    tablet_idle_for_assist = termux_target
                        .and_then(|(ip, port)| {
                            AgentClient::new(&ip, port, api_key.clone())
                                .ok()
                                .and_then(|c| c.get_telemetry().ok())
                        })
                        .map(|t| {
                            let ai_state = t.ai_status
                                .unwrap_or_else(|| "idle".to_string())
                                .to_ascii_lowercase();
                            let ai_ok = ai_state.contains("idle") || ai_state.contains("ready");
                            let cpu_ok = t.cpu_pct <= tablet_cpu_max;
                            let gpu_ok = t.gpu_pct.unwrap_or(0.0) <= tablet_gpu_max;
                            ai_ok && cpu_ok && gpu_ok
                        })
                        .unwrap_or(false);

                    last_tablet_probe = std::time::Instant::now();
                }

                let remaining = db
                    .lock()
                    .ok()
                    .and_then(|g| g.count_files_needing_media_ai().ok())
                    .unwrap_or(0);

                let urgent_env = std::env::var("THEGRID_MEDIA_URGENT")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let urgent = urgent_env || remaining >= 200;

                let mut workers: usize = match mode {
                    thegrid_ai::MediaProcessingMode::Cpu => 2,
                    thegrid_ai::MediaProcessingMode::DedicatedGpu => 6,
                    thegrid_ai::MediaProcessingMode::Auto => 3,
                };
                if urgent {
                    workers += 2;
                    batch_size = 32;
                } else {
                    batch_size = 12;
                }
                if ui_mode {
                    workers = workers.min(1);
                    batch_size = batch_size.min(4);
                }
                if tablet_assist_enabled && tablet_idle_for_assist {
                    // If tablet is free, we can run media processing harder locally while
                    // embedding load is likely shifted to tablet/offload lanes.
                    workers += 1;
                }
                workers = workers.clamp(1, 8);

                if last_mode_status.elapsed() >= std::time::Duration::from_secs(15) {
                    let _ = event_tx.send(AppEvent::Status(format!(
                        "media_mode:{} workers:{} backlog:{} tablet_assist:{}",
                        mode.as_str(),
                        workers,
                        remaining,
                        if tablet_idle_for_assist { "ready" } else { "off" }
                    )));
                    last_mode_status = std::time::Instant::now();
                }

                let batch = match db.lock() {
                    Ok(guard) => match guard.get_files_needing_media_ai(batch_size) {
                        Ok(b) => b,
                        Err(e) => {
                            log::error!("[Runtime] Media AI error in DB: {}", e);
                            break;
                        }
                    },
                    Err(_) => break,
                };

                if batch.is_empty() {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    continue;
                }

                let tablet_target = if tablet_assist_enabled
                    && tablet_idle_for_assist
                    && matches!(mode, thegrid_ai::MediaProcessingMode::DedicatedGpu)
                    && urgent
                {
                    let lock = termux_agent.lock().unwrap();
                    lock.as_ref()
                        .and_then(|a| AppRuntime::parse_agent_target(a.endpoint(), agent_port))
                } else {
                    None
                };

                let work_items: Vec<(i64, std::path::PathBuf)> = batch;
                let worker_count = workers.min(work_items.len().max(1));
                let chunk_size = ((work_items.len() + worker_count - 1) / worker_count).max(1);
                let mut handles = Vec::new();

                for chunk in work_items.chunks(chunk_size) {
                    let local_items = chunk.to_vec();
                    let local_analyzer = Arc::clone(&az);
                    let offload_target = tablet_target.clone();
                    let offload_key = api_key.clone();
                    let urgent_now = urgent;
                    let handle = std::thread::spawn(move || {
                        let mut out: Vec<(i64, Option<String>, Option<String>, bool)> = Vec::new();
                        for (fid, path) in local_items {
                            if let Some((ip, p)) = offload_target.as_ref() {
                                if let Some(remote_json) = AppRuntime::request_remote_image_analysis(
                                    &offload_key,
                                    ip,
                                    *p,
                                    fid,
                                    &path,
                                    urgent_now,
                                ) {
                                    out.push((fid, Some(remote_json), None, false));
                                    continue;
                                }
                            }

                            if path.exists() && path.is_file() {
                                match local_analyzer.analyze(&path) {
                                    Ok(meta) => {
                                        let json = serde_json::to_string(&meta).ok();
                                        out.push((fid, json, None, false));
                                    }
                                    Err(e) => {
                                        out.push((fid, None, Some(e.to_string()), false));
                                    }
                                }
                            } else {
                                out.push((fid, None, None, true));
                            }
                        }
                        out
                    });
                    handles.push(handle);
                }

                let mut merged: Vec<(i64, Option<String>, Option<String>, bool)> = Vec::new();
                for handle in handles {
                    if let Ok(mut rows) = handle.join() {
                        merged.append(&mut rows);
                    }
                }

                for (fid, json, err, delete_missing) in merged {
                    if delete_missing {
                        if let Ok(guard) = db.lock() {
                            let _ = guard.delete_file_by_id(fid);
                        }
                        continue;
                    }

                    if let Some(payload) = json {
                        if let Ok(guard) = db.lock() {
                            let _ = guard.update_ai_metadata(fid, &payload);
                            analyzed_count += 1;
                        }
                    } else if let Some(e) = err {
                        log::warn!("[Runtime] Failed to analyze media file_id={}: {}", fid, e);
                        if let Ok(guard) = db.lock() {
                            let safe = e.replace('"', "'");
                            let _ = guard.update_ai_metadata(fid, &format!("{{\"error\":\"{}\"}}", safe));
                        }
                    }

                    if analyzed_count % 5 == 0 {
                        let _ = event_tx.send(AppEvent::Status(format!(
                            "Analyzed {} media files (mode={}, workers={})",
                            analyzed_count,
                            mode.as_str(),
                            workers
                        )));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(if ui_mode { 1200 } else { 500 }));
            }

            running.store(false, Ordering::SeqCst);
        });
    }

    pub fn spawn_search_with_limit(&self, query: String, device_filter: Option<String>, semantic_enabled: bool, limit: usize) {
        let db       = Arc::clone(&self.db);
        let tx       = self.event_tx.clone();
        let semantic_search  = self.semantic_search.clone();
        let remote_ai = self.remote_ai_nodes.clone();
        let (api_key, port) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port)
        };
        let limit = limit.clamp(1, 500);

        std::thread::spawn(move || {
            let (clean_query, media_filters) = AppRuntime::parse_media_search_filters(&query);
            let effective_query = clean_query;
            let force_filtered_search = media_filters.any();
            let mut results = vec![];
            
            if semantic_enabled && !force_filtered_search {
                let has_local = {
                    let lock = semantic_search.lock().unwrap();
                    lock.is_some()
                };

                if has_local {
                    if let Ok(mut lock) = semantic_search.lock() {
                        if let Some(engine) = &mut *lock {
                            if let Ok(hits) = engine.search(&effective_query, limit) {
                                let ids: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
                                results = db.lock().ok().and_then(|guard| guard.get_files_by_ids(&ids).ok()).unwrap_or_default();
                            }
                        }
                    }
                } else {
                    // Try remote AI delegation
                    let remote_node = {
                        let nodes = remote_ai.lock().unwrap();
                        nodes.values().next().cloned()
                    };

                    if let Some(ip) = remote_node {
                        if let Ok(client) = AgentClient::new(&ip, port, api_key) {
                            if let Ok(hits) = client.remote_search(&effective_query, limit) {
                                let ids: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
                                results = db.lock().ok().and_then(|guard| guard.get_files_by_ids(&ids).ok()).unwrap_or_default();
                            }
                        }
                    }
                }
            } else {
                results = db.lock().ok().and_then(|guard| {
                    if force_filtered_search {
                        guard.search_fts_with_media_filters(
                            &effective_query,
                            limit,
                            device_filter.as_deref(),
                            media_filters.in_focus,
                            media_filters.min_quality,
                            media_filters.min_focus_score,
                            media_filters.min_megapixels,
                            media_filters.camera_contains.as_deref(),
                            media_filters.lens_contains.as_deref(),
                            media_filters.min_iso,
                            media_filters.max_iso,
                            media_filters.min_aperture,
                            media_filters.max_aperture,
                            media_filters.min_focal_mm,
                            media_filters.max_focal_mm,
                            media_filters.captured_after.as_deref(),
                            media_filters.captured_before.as_deref(),
                            media_filters.require_gps,
                            media_filters.min_rating,
                            media_filters.pick_flag.as_deref(),
                        ).ok()
                    } else {
                        guard.search_fts(&effective_query, limit, device_filter.as_deref()).ok()
                    }
                }).unwrap_or_default();
            }

            let _ = tx.send(AppEvent::SearchResults(results));
        });
    }

    pub fn spawn_search(&self, query: String, device_filter: Option<String>, semantic_enabled: bool) {
        self.spawn_search_with_limit(query, device_filter, semantic_enabled, 50);
    }

    pub fn spawn_get_telemetry(&self, ip: String, device_id: String) {
        let (port, api_key) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone())
        };
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match AgentClient::new(&ip, port, api_key).and_then(|c| c.get_telemetry()) {
                Ok(telemetry) => {
                    let _ = tx.send(AppEvent::TelemetryUpdate { 
                        device_id, 
                        ip: Some(ip), 
                        telemetry 
                    });
                }
                Err(e) => {
                    let _ = tx.send(AppEvent::Status(format!("Telemetry failed ({}): {}", device_id, e)));
                }
            }
        });
    }

    pub fn spawn_wol(&self, device_name: String, mac_addr: String) {
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let _ = WolSentry::send_multi(&mac_addr, &["255.255.255.255", "100.64.0.255"]);
            let _ = tx.send(AppEvent::WolSent { device_name, target_mac: mac_addr });
        });
    }

    pub fn spawn_load_timeline(&self, device_filter: Option<String>) {
        let db   = Arc::clone(&self.db);
        let tx   = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = db.lock().map(|guard| {
                guard.get_recent_files(200, device_filter.as_deref())
            });
            match result {
                Ok(Ok(entries)) => { let _ = tx.send(AppEvent::TemporalLoaded(entries)); }
                _ => {}
            }
        });
    }

    pub fn handle_remote_ai_embed(&self, text: String, response_tx: mpsc::Sender<Vec<f32>>) {
        let semantic = self.semantic_search.clone();
        let config = self.config.clone();
        std::thread::spawn(move || {
            {
                let lock = semantic.lock().unwrap();
                if let Some(engine) = &*lock {
                    if let Ok(v) = engine.embed(&text) {
                        let _ = response_tx.send(v);
                        return;
                    }
                }
            }

            // Fallback for nodes that changed provider at runtime or are still initializing.
            let (provider_url, model_name) = {
                let cfg = config.lock().unwrap();
                (
                    cfg.ai_provider_url.clone().unwrap_or_else(|| "http://100.67.58.127:8080".to_string()),
                    cfg.ai_model.clone().unwrap_or_else(|| "nomic-embed-text".to_string()),
                )
            };

            if let Ok(provider) = thegrid_ai::HttpEmbeddingProvider::new(model_name, provider_url) {
                if let Ok(v) = provider.embed(&text) {
                    let _ = response_tx.send(v);
                    return;
                }
            }

            if let Ok(provider) = thegrid_ai::FastEmbedProvider::new() {
                if let Ok(v) = provider.embed(&text) {
                    let _ = response_tx.send(v);
                }
            }
        });
    }

    pub fn handle_remote_ai_search(&self, query: String, k: usize, response_tx: mpsc::Sender<Vec<(i64, f32)>>) {
        let semantic = self.semantic_search.clone();
        std::thread::spawn(move || {
            let lock = semantic.lock().unwrap();
            if let Some(engine) = &*lock {
                if let Ok(hits) = engine.search(&query, k) {
                    let _ = response_tx.send(hits);
                }
            }
        });
    }

    /// Load persisted duplicate groups from the DB and emit `DuplicateGroupsRestored`.
    /// Called on startup so the DEDUP tab is populated without requiring a live scan.
    pub fn spawn_load_persisted_duplicate_groups(&self) {
        let db = Arc::clone(&self.db);
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match db.lock().map(|g| g.load_persisted_duplicate_groups()) {
                Ok(Ok((groups, actions))) => {
                    if !groups.is_empty() {
                        log::info!("[Runtime] Restored {} persisted duplicate group(s)", groups.len());
                        let _ = tx.send(AppEvent::DuplicateGroupsRestored(groups, actions));
                    }
                }
                Ok(Err(e)) => log::error!("[Runtime] Load persisted duplicate groups: {}", e),
                Err(_)     => log::error!("[Runtime] Load persisted duplicate groups: DB lock poisoned"),
            }
        });
    }

    /// Scan the DB for cross-source duplicate groups and emit `DuplicatesGrouped`.
    pub fn spawn_cross_source_dedup_scan(&self) {
        let db = Arc::clone(&self.db);
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            log::info!("[Runtime] Starting cross-source dedup scan...");
            let filter = thegrid_core::models::DuplicateScanFilter::default();
            match db.lock().map(|g| g.get_cross_source_duplicate_groups(&filter, true)) {
                Ok(Ok(groups)) => {
                    log::info!("[Runtime] Cross-source scan: {} group(s) found", groups.len());
                    let _ = tx.send(AppEvent::DuplicatesGrouped(groups));
                }
                Ok(Err(e)) => log::error!("[Runtime] Cross-source dedup scan error: {}", e),
                Err(_)     => log::error!("[Runtime] Cross-source dedup scan: DB lock poisoned"),
            }
        });
    }

    /// Run the Google Drive OAuth2 flow in a background thread.
    /// Emits `Status("drive_authorized")` on success or `DriveIndexError` on failure.
    pub fn spawn_drive_authorize(&self, client_id: String, client_secret: String) {
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            log::info!("[Runtime] Starting Google Drive authorization...");
            let mut client = DriveClient::new(client_id, client_secret);
            match client.authorize() {
                Ok(()) => {
                    log::info!("[Runtime] Google Drive authorized");
                    let _ = tx.send(AppEvent::Status("drive_authorized".to_string()));
                }
                Err(e) => {
                    log::error!("[Runtime] Drive authorization failed: {}", e);
                    let _ = tx.send(AppEvent::DriveIndexError(e.to_string()));
                }
            }
        });
    }

    /// Index Google Drive metadata in a background thread.
    /// Emits `DriveIndexProgress` / `DriveIndexComplete` / `DriveIndexError`.
    pub fn spawn_drive_index(&self, client_id: String, client_secret: String) {
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            log::info!("[Runtime] Starting Google Drive index...");
            let mut client = DriveClient::new(client_id, client_secret);
            if !client.is_authorized() {
                match client.authorize() {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[Runtime] Drive auth before index failed: {}", e);
                        let _ = tx.send(AppEvent::DriveIndexError(format!("Auth failed: {}", e)));
                        return;
                    }
                }
            }
            if let Err(e) = client.index_all_files(&tx) {
                log::error!("[Runtime] Drive index error: {}", e);
                let _ = tx.send(AppEvent::DriveIndexError(e.to_string()));
            }
        });
    }
}

// ── Media Review helpers ───────────────────────────────────────────────────

impl AppRuntime {
    /// Persist rating/pick/color for one file. Fire-and-forget — emits Status on error.
    pub fn spawn_set_media_review(
        &self,
        file_id:     i64,
        rating:      Option<u8>,
        pick_flag:   Option<String>,
        color_label: Option<String>,
    ) {
        let db = Arc::clone(&self.db);
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match db.lock().map(|g| g.set_media_review(
                file_id,
                rating,
                pick_flag.as_deref(),
                color_label.as_deref(),
            )) {
                Ok(Ok(())) => {
                    log::debug!("[Review] file {} rated={:?} pick={:?}", file_id, rating, pick_flag);
                }
                Ok(Err(e)) => {
                    let _ = tx.send(AppEvent::Status(format!("review_error:{}", e)));
                }
                Err(_) => {
                    let _ = tx.send(AppEvent::Status("review_error:db_lock_poisoned".to_string()));
                }
            }
        });
    }

    /// Load review state for one file and emit `MediaReviewLoaded`.
    pub fn spawn_get_media_review(&self, file_id: i64) {
        let db = Arc::clone(&self.db);
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            match db.lock().map(|g| g.get_media_review(file_id)) {
                Ok(Ok(Some((rating, pick_flag, color_label, reviewed_at)))) => {
                    let _ = tx.send(AppEvent::MediaReviewLoaded {
                        file_id, rating, pick_flag, color_label, reviewed_at,
                    });
                }
                Ok(Ok(None)) => {
                    let _ = tx.send(AppEvent::MediaReviewLoaded {
                        file_id,
                        rating: None,
                        pick_flag: "none".to_string(),
                        color_label: None,
                        reviewed_at: 0,
                    });
                }
                Ok(Err(e)) => {
                    let _ = tx.send(AppEvent::Status(format!("review_load_error:{}", e)));
                }
                Err(_) => {}
            }
        });
    }

    /// Load review state for a list of file_ids and emit `MediaReviewBulkLoaded`.
    pub fn spawn_load_media_review_bulk(&self, file_ids: Vec<i64>) {
        let db = Arc::clone(&self.db);
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = db.lock().map(|g| {
                let mut map = std::collections::HashMap::new();
                for id in &file_ids {
                    if let Ok(Some((rating, pick_flag, color_label, _))) = g.get_media_review(*id) {
                        map.insert(*id, (rating, pick_flag, color_label));
                    }
                }
                map
            });
            if let Ok(map) = result {
                let _ = tx.send(AppEvent::MediaReviewBulkLoaded(map));
            }
        });
    }
}
