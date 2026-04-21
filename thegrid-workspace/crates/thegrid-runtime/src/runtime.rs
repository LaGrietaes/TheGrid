use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use anyhow::Result;
use rusqlite::params;

use thegrid_core::{
    fingerprint_file, match_rules, should_skip_dir, AppEvent, Config, Database,
    DetectionSourceDistribution, FileChange, FileWatcher, SyncHealthMetrics,
};
use thegrid_net::{AgentClient, AgentServer, TailscaleClient, WolSentry};
use thegrid_ai::{SemanticSearch, EmbeddingProvider, AiNodeDetector};

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
    pub event_tx:     mpsc::Sender<AppEvent>,
    
    // Services
    pub file_watcher:    Arc<Mutex<Option<FileWatcher>>>,
    pub semantic_search: Arc<Mutex<Option<SemanticSearch>>>,
    
    // Remote AI Capabilities (device_id -> ip)
    pub remote_ai_nodes: Arc<Mutex<std::collections::HashMap<String, String>>>,
    
    // State
    pub is_ai_node: bool,
    pub media_analyzer: Arc<Mutex<Option<Arc<dyn thegrid_ai::MediaAnalyzer>>>>,
    pub agent_shutdown: Arc<Mutex<Option<Arc<AtomicBool>>>>,
    pub hash_worker_running: Arc<AtomicBool>,
    pub sync_health: Arc<Mutex<std::collections::HashMap<String, SyncHealthMetrics>>>,
}

impl AppRuntime {
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

        let runtime = Self {
            config: Arc::new(Mutex::new(config)),
            db,
            event_tx,
            file_watcher,
            semantic_search: Arc::new(Mutex::new(None)),
            remote_ai_nodes: Arc::new(Mutex::new(std::collections::HashMap::new())),
            is_ai_node,
            media_analyzer: Arc::new(Mutex::new(None)),
            agent_shutdown: Arc::new(Mutex::new(None)),
            hash_worker_running: Arc::new(AtomicBool::new(false)),
            sync_health: Arc::new(Mutex::new(std::collections::HashMap::new())),
        };

        Ok(runtime)
    }

    pub fn start_services(&self) {
        let (p, k, c) = {
            let cfg = self.config.lock().unwrap();
            (cfg.agent_port, cfg.api_key.clone(), cfg.clone())
        };

        // Start agent server
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

        // Start AI if capable OR if provider URL is set (which overrides local specs)
        let has_remote_provider = {
            let cfg = self.config.lock().unwrap();
            cfg.ai_provider_url.is_some()
        };

        if self.is_ai_node || has_remote_provider {
            self.spawn_semantic_initializer();
            self.spawn_embedding_worker();
        }

        // Phase 4: Media AI for GPU nodes
        if self.is_ai_node {
            if let Ok(analyzer) = thegrid_ai::CudaMediaAnalyzer::new() {
                let mut lock = self.media_analyzer.lock().unwrap();
                *lock = Some(Arc::new(analyzer));
                self.spawn_media_analyzer_worker();
            }
        }

        // Background indexing helpers
        self.spawn_hashing_worker();

        let mut shutdown_lock = self.agent_shutdown.lock().unwrap();
        *shutdown_lock = Some(server.shutdown_handle());
        server.spawn();
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
            let _ = tx.send(AppEvent::Status(format!("Calculating total files for {}...", path.display())));

            // Pass 1: Quick count over the entire tree
            let total_count = jwalk::WalkDir::new(&path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .count() as u64;

            log::info!("[Runtime] Total files to index in {:?}: {}", path, total_count);
            let _ = tx.send(AppEvent::Status(format!("Indexing {} files...", total_count)));

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
            // We pass the total_count as a hint for the progress bar
            Self::do_process_index_queue(
                db, 
                config,
                tx, 
                device_id, 
                device_name, 
                total_count,
                Some(path.to_string_lossy().to_string())
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
        let mut scanned_count = 0u64;

        loop {
            let task = {
                let guard = db.lock().unwrap();
                guard
                    .get_next_index_task_for_root(root_filter.as_deref())
                    .unwrap_or(None)
            };

            if let Some((root, dir_str)) = task {
                let dir_path = PathBuf::from(&dir_str);
                log::info!("[Runtime] Persistent scan: {:?}", dir_path);

                // Parse file-system entries outside DB lock to reduce contention
                let entries = match std::fs::read_dir(&dir_path) {
                    Ok(e) => e,
                    Err(_) => {
                        if let Ok(guard) = db.lock() {
                            let _ = guard.complete_index_task(&root, &dir_str);
                        }
                        continue;
                    }
                };

                let cfg_snapshot = config.lock().ok().map(|c| c.clone());
                let mut subdirs: Vec<PathBuf> = Vec::new();
                let mut files_to_index: Vec<(PathBuf, u64, Option<i64>, Option<String>)> = Vec::new();

                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path();
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
                match db.lock() {
                    Ok(guard) => {
                        for (path, size, modified, quick_hash) in &files_to_index {
                            if let Ok(fid) = guard.index_file_with_source(
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
                                        &guard,
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
                            let _ = guard.conn.execute(
                                "INSERT OR IGNORE INTO index_queue (root_path, dir_path) VALUES (?, ?)",
                                params![root.as_str(), s.to_string_lossy()]
                            );
                        }

                        let _ = guard.complete_index_task(&root, &dir_str);
                        scanned_count += dir_count;

                        let progress_scanned = if total_hint > 0 {
                            scanned_count.min(total_hint)
                        } else {
                            scanned_count
                        };

                        let _ = tx.send(AppEvent::IndexProgress {
                            scanned: progress_scanned,
                            total:   total_hint,
                            current: dir_path.file_name().unwrap_or_default().to_string_lossy().into(),
                            ext:     None,
                        });
                    }
                    Err(_) => break,
                }
            } else {
                break;
            }
        }

        let _ = tx.send(AppEvent::IndexComplete {
            device_id,
            files_added: scanned_count,
            duration_ms: start.elapsed().as_millis() as u64,
        });

        // Background worker triggers could go here
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

            if count > 0 {
                let _ = event_tx.send(AppEvent::SyncComplete { device_id, files_added: count });
            }
        });
    }

    pub fn spawn_semantic_initializer(&self) {
        let event_tx = self.event_tx.clone();
        let db       = self.db.clone();
        let semantic = self.semantic_search.clone();
        let config   = self.config.clone();
        
        std::thread::spawn(move || {
            let (model, url) = {
                let cfg = config.lock().unwrap();
                (cfg.ai_model.clone(), cfg.ai_provider_url.clone())
            };

            let provider: Result<Arc<dyn EmbeddingProvider>> = if let Some(u) = url {
                let m = model.unwrap_or_else(|| "llama3".to_string());
                log::info!("[AI] Using remote HTTP provider {} at {}", m, u);
                Ok(Arc::new(thegrid_ai::HttpEmbeddingProvider::new(m, u)))
            } else {
                log::info!("[AI] Initializing local provider...");
                thegrid_ai::FastEmbedProvider::new().map(|p| Arc::new(p) as Arc<dyn EmbeddingProvider>)
            };

            match provider {
                Ok(p) => {
                    match SemanticSearch::new(p) {
                        Ok(mut search) => {
                            if let Ok(guard) = db.lock() {
                                if let Ok(all) = guard.get_all_embeddings() {
                                    for (fid, vec) in all {
                                        let _ = search.add_vector(fid, &vec);
                                    }
                                }
                            }
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
                    let _ = event_tx.send(AppEvent::SemanticFailed(format!("Provider init: {}", e)));
                }
            }
        });
    }

    pub fn spawn_embedding_worker(&self) {
        let db       = self.db.clone();
        let event_tx = self.event_tx.clone();
        let semantic = self.semantic_search.clone();
        let remote_ai = self.remote_ai_nodes.clone();
        let (api_key, port) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port)
        };
        
        std::thread::spawn(move || {
            let total = db.lock().ok().and_then(|g| g.count_unindexed_files().ok()).unwrap_or(0);
            let mut indexed = 0;

            loop {
                let has_local_engine = {
                    let lock = semantic.lock().expect("semantic lock");
                    lock.is_some()
                };

                let batch = match db.lock() {
                    Ok(guard) => guard.get_files_needing_embedding(20).unwrap_or_default(),
                    Err(_) => break,
                };
                
                if batch.is_empty() { 
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }
                
                for (fid, text) in batch {
                    let mut success = false;
                    let mut vector = vec![];

                    if has_local_engine {
                        let mut lock = semantic.lock().expect("semantic lock");
                        if let Some(search) = &mut *lock {
                            if let Ok(v) = search.index_file(fid, &text) {
                                vector = v;
                                success = true;
                            }
                        }
                    } else {
                        // Try remote AI delegation
                        let remote_node = {
                            let nodes = remote_ai.lock().unwrap();
                            nodes.values().next().cloned() // Just take the first one for now
                        };
                        
                        if let Some(ip) = remote_node {
                            if let Ok(client) = AgentClient::new(&ip, port, api_key.clone()) {
                                if let Ok(v) = client.remote_embed(&text) {
                                    vector = v;
                                    success = true;
                                }
                            }
                        }
                    }

                    if success {
                        if let Ok(db_lock) = db.lock() {
                            let _ = db_lock.save_embedding(fid, "delegated", &vector);
                        }
                    }
                    indexed += 1;
                    let _ = event_tx.send(AppEvent::EmbeddingProgress { indexed, total });
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
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

        std::thread::spawn(move || {
            log::info!("[Runtime] Starting background hashing worker...");
            let batch_size = 50;
            let mut hashed_count = 0;

            loop {
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
                    // Nothing to hash right now, wait and check again later (or exit if not needed anymore)
                    std::thread::sleep(std::time::Duration::from_secs(10));
                    continue;
                }

                for (fid, path) in batch {
                    if path.exists() && path.is_file() {
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
                                    hashed_count += 1;
                                }
                            }
                            Err(e) => {
                                log::warn!("[Runtime] Failed to hash {:?}: {}", path, e);
                                // Mark as failed in DB so we don't retry forever
                                if let Ok(guard) = db.lock() {
                                    let _ = guard.update_file_hash(fid, &format!("ERR_{}", e));
                                }
                            }
                        }
                    } else {
                        // File gone, remove from DB or mark as missing
                        if let Ok(guard) = db.lock() {
                            let _ = guard.delete_file_by_id(fid);
                        }
                    }
                    
                    if hashed_count % 10 == 0 {
                        let _ = event_tx.send(AppEvent::Status(format!("Hashed {} files", hashed_count)));
                    }
                }
                
                // Yield to other threads
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            running.store(false, Ordering::SeqCst);
        });
    }

    pub fn spawn_media_analyzer_worker(&self) {
        let db = Arc::clone(&self.db);
        let event_tx = self.event_tx.clone();
        let analyzer = Arc::clone(&self.media_analyzer);

        std::thread::spawn(move || {
            log::info!("[Runtime] Starting background media analyzer worker...");
            let batch_size = 10;
            let mut analyzed_count = 0;

            loop {
                let az = {
                    let lock = analyzer.lock().unwrap();
                    lock.clone()
                };

                if az.is_none() {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    continue;
                }
                let az = az.unwrap();

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

                for (fid, path) in batch {
                    if path.exists() && path.is_file() {
                        match az.analyze(&path) {
                            Ok(meta) => {
                                if let Ok(json) = serde_json::to_string(&meta) {
                                    if let Ok(guard) = db.lock() {
                                        let _ = guard.update_ai_metadata(fid, &json);
                                        analyzed_count += 1;
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("[Runtime] Failed to analyze media {:?}: {}", path, e);
                                if let Ok(guard) = db.lock() {
                                    let _ = guard.update_ai_metadata(fid, &format!("{{\"error\":\"{}\"}}", e));
                                }
                            }
                        }
                    } else {
                        if let Ok(guard) = db.lock() {
                            let _ = guard.delete_file_by_id(fid);
                        }
                    }

                    if analyzed_count % 5 == 0 {
                        let _ = event_tx.send(AppEvent::Status(format!("Analyzed {} media files with GPU", analyzed_count)));
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
    }

    pub fn spawn_search(&self, query: String, device_filter: Option<String>, semantic_enabled: bool) {
        let db       = Arc::clone(&self.db);
        let tx       = self.event_tx.clone();
        let semantic_search  = self.semantic_search.clone();
        let remote_ai = self.remote_ai_nodes.clone();
        let (api_key, port) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port)
        };

        std::thread::spawn(move || {
            let mut results = vec![];
            
            if semantic_enabled {
                let has_local = {
                    let lock = semantic_search.lock().unwrap();
                    lock.is_some()
                };

                if has_local {
                    if let Ok(mut lock) = semantic_search.lock() {
                        if let Some(engine) = &mut *lock {
                            if let Ok(hits) = engine.search(&query, 50) {
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
                            if let Ok(hits) = client.remote_search(&query, 50) {
                                let ids: Vec<i64> = hits.iter().map(|(id, _)| *id).collect();
                                results = db.lock().ok().and_then(|guard| guard.get_files_by_ids(&ids).ok()).unwrap_or_default();
                            }
                        }
                    }
                }
            } else {
                results = db.lock().ok().and_then(|guard| {
                    guard.search_fts(&query, 50, device_filter.as_deref()).ok()
                }).unwrap_or_default();
            }

            let _ = tx.send(AppEvent::SearchResults(results));
        });
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
        std::thread::spawn(move || {
            let lock = semantic.lock().unwrap();
            if let Some(engine) = &*lock {
                if let Ok(v) = engine.embed(&text) {
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
}
