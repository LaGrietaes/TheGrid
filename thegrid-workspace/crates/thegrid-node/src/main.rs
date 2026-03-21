use anyhow::Result;
use std::path::PathBuf;
use std::sync::mpsc;
use thegrid_core::{AppEvent, Config};
use thegrid_runtime::AppRuntime;

fn main() -> Result<()> {
    // Initialize logging
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    ).init();

    log::info!("THE GRID Headless Node v{} starting", env!("CARGO_PKG_VERSION"));

    // 1. Load Config
    let mut config = Config::load().unwrap_or_else(|e| {
        log::warn!("Failed to load config: {}. Using default.", e);
        Config::default()
    });

    // Support environment variables for headless setup
    if let Ok(key) = std::env::var("THEGRID_API_KEY") {
        config.api_key = key;
    }
    if let Ok(name) = std::env::var("THEGRID_DEVICE_NAME") {
        config.device_name = name;
    }

    // Support simple CLI arguments: --api-key <key> --device-name <name>
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--api-key" && i + 1 < args.len() {
            config.api_key = args[i+1].clone();
        }
        if args[i] == "--device-name" && i + 1 < args.len() {
            config.device_name = args[i+1].clone();
        }
    }

    if config.device_name.is_empty() {
        config.device_name = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "UNKNOWN-NODE".to_string());
    }

    // 2. Initialize Runtime
    let (tx, rx) = mpsc::channel::<AppEvent>();
    let runtime = AppRuntime::new(config, tx)?;
    runtime.start_services();

    log::info!("Node is running. Press Ctrl+C to stop.");

    // Simple loop to handle events
    loop {
        // Drain events
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::SyncRequest { after, response_tx } => {
                    log::info!("Incoming sync request (after timestamp: {})", after);
                    if let Ok(guard) = runtime.db.lock() {
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
                    runtime.spawn_incremental_index(paths);
                }
                AppEvent::ClipboardReceived(entry) => {
                    log::info!("Clipboard received from {}: {}", entry.sender, entry.content);
                    
                    // Attempt to set Termux clipboard if running on Android
                    #[cfg(target_os = "linux")]
                    {
                        if let Err(e) = std::process::Command::new("termux-clipboard-set")
                            .arg(&entry.content)
                            .output() 
                        {
                            log::debug!("Failed to set termux clipboard (might not be Termux): {}", e);
                        }
                    }
                }
                AppEvent::FileReceived { name, size } => {
                    log::info!("File received: {} ({} bytes)", name, size);
                }
                AppEvent::Status(msg) => {
                    log::debug!("Status: {}", msg);
                }
                AppEvent::EnableAdb { .. } => {
                    log::info!("Enabling ADB over TCP/IP (port 5555)...");
                    let _ = std::process::Command::new("adb")
                        .arg("tcpip")
                        .arg("5555")
                        .spawn();
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
