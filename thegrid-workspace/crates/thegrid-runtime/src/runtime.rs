use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use anyhow::Result;

use thegrid_core::{AppEvent, Config, Database, FileWatcher, hash_file, match_rules};
use thegrid_net::{AgentClient, AgentServer, TailscaleClient, WolSentry};
use thegrid_ai::{SemanticSearch, EmbeddingProvider, AiNodeDetector};

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
    pub agent_shutdown: Arc<Mutex<Option<Arc<AtomicBool>>>>,
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
                log::error!("Failed to open database: {} — using in-memory fallback", e);
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
            agent_shutdown: Arc::new(Mutex::new(None)),
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
        }

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
        let tx = self.event_tx.clone();

        std::thread::Builder::new()
            .name(format!("tg-index-{}", path.display()))
            .spawn(move || {
                let start = std::time::Instant::now();
                let result = {
                    match db.lock() {
                        Err(_) => { let _ = tx.send(AppEvent::Status("Index lock failed".into())); return; }
                        Ok(guard) => {
                            guard.index_directory(
                                &device_id,
                                &device_name,
                                &path,
                                |scanned, current| {
                                    if scanned % 250 == 0 {
                                        let current_str = current.file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string();
                                        let _ = tx.send(AppEvent::IndexProgress {
                                            scanned,
                                            total: scanned + 1,
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

    pub fn spawn_incremental_index(&self, paths: Vec<PathBuf>) {
        let db = Arc::clone(&self.db);
        let (device_id, device_name) = {
            let cfg = self.config.lock().unwrap();
            (cfg.device_name.clone(), cfg.device_name.clone())
        };
        let tx = self.event_tx.clone();
        std::thread::spawn(move || {
            let result = db.lock().map(|guard| {
                guard.index_changed_paths(&device_id, &device_name, &paths)
            });
            match result {
                Ok(Ok((updated, _deleted))) => {
                    let _ = tx.send(AppEvent::IndexUpdated { paths_updated: updated });
                }
                _ => {}
            }
        });
    }

    pub fn spawn_sync_node(&self, device_id: String, ip: String, hostname: String) {
        let db          = self.db.clone();
        let event_tx    = self.event_tx.clone();
        let (api_key, port) = {
            let cfg = self.config.lock().unwrap();
            (cfg.api_key.clone(), cfg.agent_port)
        };

        std::thread::spawn(move || {
            let last_ts = match db.lock() {
                Ok(guard) => guard.get_node_sync_ts(&device_id).unwrap_or(0),
                Err(_)    => 0,
            };

            if let Ok(client) = AgentClient::new(&ip, port, api_key) {
                if let Ok(results) = client.sync_index(last_ts) {
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
                
                if batch.is_empty() { break; }
                
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

    pub fn spawn_background_hasher(&self) {
        let db       = self.db.clone();
        let event_tx = self.event_tx.clone();
        
        std::thread::spawn(move || {
            log::info!("[Runtime] Starting background hashing worker...");
            let mut hashed = 0;

            loop {
                // Fetch a batch of files needing a hash
                let batch = match db.lock() {
                    Ok(guard) => guard.get_files_needing_hash(50).unwrap_or_default(),
                    Err(_) => break,
                };
                
                if batch.is_empty() { break; }
                
                for (fid, path) in batch {
                    if path.exists() && path.is_file() {
                        let hash_res = hash_file(&path);
                        if let Ok(db_lock) = db.lock() {
                            match hash_res {
                                Ok(h) => {
                                    let _ = db_lock.update_file_hash(fid, &h);
                                    
                                    // NEW: Apply Smart Filter Rules
                                    if let Ok(rules) = db_lock.get_rules() {
                                        let user_rules: Vec<_> = rules.into_iter().map(|r| {
                                            thegrid_core::models::UserRule {
                                                id: r.0, name: r.1, pattern: r.2, project: r.3, tag: r.4, is_active: r.5
                                            }
                                        }).collect();
                                        
                                        let matches = match_rules(&path, &user_rules, fid);
                                        for m in matches {
                                            let _ = db_lock.add_file_tag(fid, m.tag.as_deref(), m.project.as_deref(), false);
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!("[Runtime] Failed to hash {:?}: {}", path, e);
                                    let _ = db_lock.update_file_hash(fid, "ERR_HASH_FAILED");
                                }
                            }
                        }
                    } else {
                        if let Ok(db_lock) = db.lock() {
                            let _ = db_lock.update_file_hash(fid, "ERR_NOT_FOUND");
                        }
                    }
                    hashed += 1;
                    if hashed % 25 == 0 {
                        // Total is hard to estimate exactly, we just send current count
                        let _ = event_tx.send(AppEvent::HashingProgress { hashed, total: hashed + 50 });
                    }
                }
                
                // Throttle to keep CPU usage reasonable
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            log::info!("[Runtime] Background hashing worker finished.");
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
