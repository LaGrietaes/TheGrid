//! TheGrid autonomous agents — Sentinel, Librarian, Courier.
//!
//! Each agent runs in its own background thread, communicates via the shared
//! `AppEvent` mpsc bus, and uses only local inference by default.
//!
//! Architecture:
//!   - `Sentinel`  — security stance management and anomaly detection.
//!   - `Librarian` — distributed semantic search coordinator.
//!   - `Courier`   — chunked, resumable large-file transfers (VerteX protocol).
//!
//! All agents respect the `ai_policy` gate:  cloud calls are disabled under
//! "local_only" and only enabled when the operator explicitly opts in.

use anyhow::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use thegrid_core::{AppEvent, Config, models::SecurityStance};
use crate::inference::InferenceProvider;

// ── Shared helpers ────────────────────────────────────────────────────────────

fn emit(tx: &mpsc::Sender<AppEvent>, event: AppEvent) {
    if let Err(e) = tx.send(event) {
        log::warn!("[Agent] Failed to emit event: {}", e);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SENTINEL
// Security stance gatekeeper and anomaly detector.
// ─────────────────────────────────────────────────────────────────────────────

pub struct Sentinel {
    config:     Arc<Mutex<Config>>,
    event_tx:   mpsc::Sender<AppEvent>,
    inference:  Option<Arc<dyn InferenceProvider>>,

    /// Current security stance. Starts Active.
    stance:      Mutex<SecurityStance>,

    /// Last time any UI activity was observed.
    last_active: Mutex<Instant>,

    /// Threshold before AFK lock kicks in (default 10 min).
    afk_threshold: Duration,
}

impl Sentinel {
    pub fn new(
        config: Arc<Mutex<Config>>,
        event_tx: mpsc::Sender<AppEvent>,
        inference: Option<Arc<dyn InferenceProvider>>,
    ) -> Arc<Self> {
        let afk_threshold = Duration::from_secs(10 * 60);
        Arc::new(Self {
            config,
            event_tx,
            inference,
            stance: Mutex::new(SecurityStance::Active),
            last_active: Mutex::new(Instant::now()),
            afk_threshold,
        })
    }

    /// Signal that the user is active (call on any UI interaction).
    pub fn user_active(&self) {
        *self.last_active.lock().unwrap() = Instant::now();
        self.transition_to(SecurityStance::Active);
    }

    /// Force AFK tactical lock immediately (used by GUI idle detector).
    pub fn lock_afk(&self) {
        self.transition_to(SecurityStance::AfkTacticalLock);
    }

    /// Trigger a high-value-target lock (call before destructive operations).
    pub fn request_hvt_lock(&self, action_description: String) {
        self.transition_to(SecurityStance::HighValueTarget { action_description });
    }

    fn current_stance(&self) -> SecurityStance {
        self.stance.lock().unwrap().clone()
    }

    fn transition_to(&self, new_stance: SecurityStance) {
        let mut guard = self.stance.lock().unwrap();
        if *guard != new_stance {
            log::info!("[Sentinel] Stance: {:?} → {:?}", *guard, new_stance);
            *guard = new_stance.clone();
            drop(guard);
            emit(&self.event_tx, AppEvent::SecurityStanceChanged(new_stance));
        }
    }

    /// Background loop: check for AFK, run anomaly heuristics.
    /// Spawn this in a dedicated thread via `Sentinel::spawn()`.
    pub fn run_loop(self: Arc<Self>) {
        log::info!("[Sentinel] Started.");
        loop {
            std::thread::sleep(Duration::from_secs(30));

            // AFK check
            let idle = self.last_active.lock().unwrap().elapsed();
            let current = self.current_stance();
            if idle >= self.afk_threshold && current == SecurityStance::Active {
                self.transition_to(SecurityStance::AfkTacticalLock);
                emit(&self.event_tx, AppEvent::AgentAlert {
                    agent:   "sentinel".into(),
                    level:   "info".into(),
                    message: format!("AFK lock engaged after {:.0?} idle.", idle),
                });
            }

            // Placeholder: future anomaly detection hook.
            // When inference is available, run a lightweight heuristic prompt
            // against recent AppEvent summaries to detect anomalous patterns.
        }
    }

    pub fn spawn(agent: Arc<Self>) {
        std::thread::Builder::new()
            .name("agent-sentinel".into())
            .spawn(move || agent.run_loop())
            .expect("Failed to spawn Sentinel thread");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// LIBRARIAN
// Distributed semantic search coordinator.
// Manages the local vector index and fans out queries to peer nodes.
// ─────────────────────────────────────────────────────────────────────────────

pub struct Librarian {
    config:    Arc<Mutex<Config>>,
    event_tx:  mpsc::Sender<AppEvent>,
    inference: Option<Arc<dyn InferenceProvider>>,
}

impl Librarian {
    pub fn new(
        config: Arc<Mutex<Config>>,
        event_tx: mpsc::Sender<AppEvent>,
        inference: Option<Arc<dyn InferenceProvider>>,
    ) -> Arc<Self> {
        Arc::new(Self { config, event_tx, inference })
    }

    /// Execute a distributed search across local index + available mesh peers.
    ///
    /// - `local_results`: pre-computed from the local `SemanticSearch` instance.
    /// - `peer_clients`: map of device_id → AgentClient for each reachable peer.
    ///
    /// Emits `AppEvent::LibrarianSearchResult` on completion.
    pub fn search_distributed(
        &self,
        query: String,
        local_results: Vec<(i64, f32)>,
        peer_clients: HashMap<String, Arc<dyn PeerSearchClient>>,
        k: usize,
    ) {
        let event_tx = self.event_tx.clone();
        let q = query.clone();

        std::thread::Builder::new()
            .name("agent-librarian-search".into())
            .spawn(move || {
                let mut peer_results: HashMap<String, Vec<(i64, f32)>> = HashMap::new();

                for (device_id, client) in &peer_clients {
                    match client.semantic_search(&q, k) {
                        Ok(results) => {
                            peer_results.insert(device_id.clone(), results);
                        }
                        Err(e) => {
                            log::warn!("[Librarian] Peer {} search failed: {}", device_id, e);
                        }
                    }
                }

                emit(&event_tx, AppEvent::LibrarianSearchResult {
                    query,
                    results: local_results,
                    peer_results,
                });
            })
            .expect("Failed to spawn Librarian search thread");
    }

    /// Optionally expand a terse query into a more semantic form using local inference.
    /// Returns the original query if inference is unavailable.
    pub fn expand_query(&self, query: &str) -> String {
        let Some(inf) = &self.inference else {
            return query.to_string();
        };

        let system = "You expand terse search queries into a richer semantic version. \
                      Return only the expanded query, nothing else. Keep it under 30 words.";
        match inf.complete(Some(system), query) {
            Ok(expanded) => {
                log::debug!("[Librarian] Query expanded: {:?} → {:?}", query, expanded);
                expanded
            }
            Err(e) => {
                log::warn!("[Librarian] Query expansion failed: {}. Using original.", e);
                query.to_string()
            }
        }
    }
}

/// Thin interface so Librarian can call peers without depending on thegrid-net directly.
pub trait PeerSearchClient: Send + Sync {
    fn semantic_search(&self, query: &str, k: usize) -> Result<Vec<(i64, f32)>>;
}

// ─────────────────────────────────────────────────────────────────────────────
// COURIER
// VerteX protocol — chunked, resumable large-file transfer.
// ─────────────────────────────────────────────────────────────────────────────

/// Default chunk size: 4 MiB — balance between network efficiency and resume granularity.
const DEFAULT_CHUNK_BYTES: u64 = 4 * 1024 * 1024;

/// State for one in-flight VerteX transfer.
#[derive(Debug, Clone)]
pub struct CourierTransfer {
    pub id:          String,
    pub file_name:   String,
    pub file_path:   std::path::PathBuf,
    pub peer_device: String,
    pub peer_url:    String,
    pub total_bytes: u64,
    pub bytes_sent:  u64,
    pub chunk_size:  u64,
}

impl CourierTransfer {
    pub fn progress_pct(&self) -> f32 {
        if self.total_bytes == 0 { return 0.0; }
        (self.bytes_sent as f32 / self.total_bytes as f32) * 100.0
    }
}

pub struct Courier {
    config:    Arc<Mutex<Config>>,
    event_tx:  mpsc::Sender<AppEvent>,
    /// Active transfers keyed by transfer_id.
    transfers: Mutex<HashMap<String, CourierTransfer>>,
}

impl Courier {
    pub fn new(config: Arc<Mutex<Config>>, event_tx: mpsc::Sender<AppEvent>) -> Arc<Self> {
        Arc::new(Self {
            config,
            event_tx,
            transfers: Mutex::new(HashMap::new()),
        })
    }

    /// Enqueue and begin a VerteX transfer.
    ///
    /// The transfer runs in a background thread and emits `CourierProgress`,
    /// `CourierComplete`, or `CourierFailed` events.
    pub fn send(
        self: &Arc<Self>,
        file_path: std::path::PathBuf,
        peer_device: String,
        peer_url: String,
    ) -> Result<String> {
        use std::io::Read;

        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let meta = std::fs::metadata(&file_path)
            .map_err(|e| anyhow::anyhow!("Cannot stat {:?}: {}", file_path, e))?;
        let total_bytes = meta.len();

        let transfer_id = format!(
            "vtx-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            &file_name[..file_name.len().min(8)]
        );

        let transfer = CourierTransfer {
            id:          transfer_id.clone(),
            file_name:   file_name.clone(),
            file_path:   file_path.clone(),
            peer_device: peer_device.clone(),
            peer_url:    peer_url.clone(),
            total_bytes,
            bytes_sent:  0,
            chunk_size:  DEFAULT_CHUNK_BYTES,
        };

        self.transfers.lock().unwrap().insert(transfer_id.clone(), transfer.clone());

        let event_tx  = self.event_tx.clone();
        let transfers = Arc::clone(&Arc::new(Mutex::new(
            self.transfers.lock().unwrap().clone()
        )));

        let tid = transfer_id.clone();
        std::thread::Builder::new()
            .name(format!("agent-courier-{}", &transfer_id[..12]))
            .spawn(move || {
                log::info!("[Courier] Starting VerteX transfer {} → {} ({}B)", tid, peer_device, total_bytes);

                let client = match reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(120))
                    .build()
                {
                    Ok(c) => c,
                    Err(e) => {
                        emit(&event_tx, AppEvent::CourierFailed {
                            transfer_id: tid,
                            error: format!("HTTP client build failed: {}", e),
                        });
                        return;
                    }
                };

                let mut file = match std::fs::File::open(&file_path) {
                    Ok(f) => f,
                    Err(e) => {
                        emit(&event_tx, AppEvent::CourierFailed {
                            transfer_id: tid,
                            error: format!("Cannot open {:?}: {}", file_path, e),
                        });
                        return;
                    }
                };

                let mut offset: u64 = 0;
                let mut buf = vec![0u8; DEFAULT_CHUNK_BYTES as usize];

                loop {
                    let n = match file.read(&mut buf) {
                        Ok(0) => break, // EOF
                        Ok(n) => n,
                        Err(e) => {
                            emit(&event_tx, AppEvent::CourierFailed {
                                transfer_id: tid,
                                error: format!("Read error at offset {}: {}", offset, e),
                            });
                            return;
                        }
                    };

                    let chunk = &buf[..n];
                    let url = format!("{}/vertex/upload", peer_url.trim_end_matches('/'));

                    let resp = client
                        .post(&url)
                        .header("X-VerteX-TransferId", &tid)
                        .header("X-VerteX-FileName",   &file_name)
                        .header("X-VerteX-Offset",     offset.to_string())
                        .header("X-VerteX-Total",      total_bytes.to_string())
                        .body(chunk.to_vec())
                        .send();

                    match resp {
                        Ok(r) if r.status().is_success() => {
                            offset += n as u64;
                            emit(&event_tx, AppEvent::CourierProgress {
                                transfer_id: tid.clone(),
                                file_name:   file_name.clone(),
                                bytes_done:  offset,
                                bytes_total: total_bytes,
                                peer_device: peer_device.clone(),
                            });
                        }
                        Ok(r) => {
                            emit(&event_tx, AppEvent::CourierFailed {
                                transfer_id: tid,
                                error: format!("Peer rejected chunk at offset {}: HTTP {}", offset, r.status()),
                            });
                            return;
                        }
                        Err(e) => {
                            emit(&event_tx, AppEvent::CourierFailed {
                                transfer_id: tid,
                                error: format!("Network error at offset {}: {}", offset, e),
                            });
                            return;
                        }
                    }
                }

                emit(&event_tx, AppEvent::CourierComplete {
                    transfer_id: tid,
                    file_name,
                    peer_device,
                });
            })
            .expect("Failed to spawn Courier thread");

        Ok(transfer_id)
    }
}
