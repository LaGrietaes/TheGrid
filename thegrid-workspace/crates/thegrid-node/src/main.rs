use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use thegrid_core::{AppEvent, Config, Database, FileWatcher};
use thegrid_net::AgentServer;

fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    log::info!("THE GRID Headless Node v{} starting", env!("CARGO_PKG_VERSION"));

    // 1. Load Config
    let config = Config::load().unwrap_or_else(|e| {
        log::warn!("Failed to load config: {}. Using default.", e);
        Config::default()
    });

    // 2. Open Database
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
            Arc::new(Mutex::new(
                Database::open(":memory:").expect("In-memory DB must always work")
            ))
        }
    };

    let (tx, rx) = mpsc::channel::<AppEvent>();

    // 3. Start Agent Server (Port 8080 by default)
    let transfers_dir = config.effective_transfers_dir();
    AgentServer::new(
        config.agent_port,
        config.api_key.clone(),
        transfers_dir.clone(),
        tx.clone()
    ).spawn();

    // 4. Start Filesystem Watcher
    let mut file_watcher = match FileWatcher::new(tx.clone()) {
        Ok(fw) => { log::info!("FileWatcher ready"); Some(fw) }
        Err(e) => { log::warn!("FileWatcher unavailable: {}", e); None }
    };

    // Initialize with watch paths from config
    if let Some(fw) = &mut file_watcher {
        for path in &config.watch_paths {
            log::info!("Watching: {:?}", path);
            let _ = fw.watch(path.clone());
        }
    }

    log::info!("Node is running. Press Ctrl+C to stop.");

    // Simple loop to handle events
    // In a future version, we could use tokio for better async handling,
    // but the current agent server uses std threads and mpsc.
    loop {
        // Drain events
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::SyncRequest { after, response_tx } => {
                    log::info!("Incoming sync request (after timestamp: {})", after);
                    if let Ok(guard) = db.lock() {
                        match guard.get_files_after(after) {
                            Ok(results) => {
                                let _ = response_tx.send(results);
                            }
                            Err(e) => {
                                log::error!("Failed to query files for sync: {}", e);
                                let _ = response_tx.send(vec![]);
                            }
                        }
                    }
                }
                AppEvent::FileSystemChanged { paths, summary } => {
                    log::info!("FileSystem Changed: {}", summary);
                    if let Ok(guard) = db.lock() {
                        let dev_id = config.device_name.clone();
                        let dev_name = config.device_name.clone();
                        if let Err(e) = guard.index_changed_paths(&dev_id, &dev_name, &paths) {
                            log::error!("Incremental index failed: {}", e);
                        }
                    }
                }
                AppEvent::ClipboardReceived(entry) => {
                    log::info!("Clipboard received from {}: {}", entry.sender, entry.content);
                    // In headless mode, we might just log it or save to a file
                }
                AppEvent::FileReceived { name, size } => {
                    log::info!("File received: {} ({} bytes)", name, size);
                }
                AppEvent::Status(msg) => {
                    log::debug!("Status: {}", msg);
                }
                _ => {
                    // Ignore GUI events
                    log::debug!("Ignored event: {:?}", event);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
