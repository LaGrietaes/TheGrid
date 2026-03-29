use anyhow::Result;
use chrono::Local;
use semver::Version;
use serde::Deserialize;

use std::io::{self, IsTerminal, Write};
use std::process::Command;
use std::sync::mpsc;
use thegrid_core::{AppEvent, Config};
use thegrid_runtime::AppRuntime;

const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/LaGrietaes/TheGrid/releases/latest";

#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    html_url: String,
}

fn parse_version_tag(tag: &str) -> Option<Version> {
    let clean = tag.trim().trim_start_matches('v').trim_start_matches('V');
    Version::parse(clean).ok()
}

fn check_latest_release() -> Result<Option<ReleaseInfo>> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    let release = client
        .get(RELEASES_LATEST_URL)
        .header("User-Agent", format!("thegrid-node/{}", env!("CARGO_PKG_VERSION")))
        .send()?
        .error_for_status()?
        .json::<ReleaseInfo>()?;

    let latest = match parse_version_tag(&release.tag_name) {
        Some(v) => v,
        None => return Ok(None),
    };

    if latest > current {
        Ok(Some(release))
    } else {
        Ok(None)
    }
}

fn prompt_yes_no(prompt: &str) -> bool {
    print!("{}", prompt);
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn try_git_update() -> Result<()> {
    let probe = Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .output()?;

    if !probe.status.success() {
        anyhow::bail!("Current directory is not a git repository");
    }

    let pull = Command::new("git")
        .arg("pull")
        .arg("--ff-only")
        .output()?;

    if !pull.status.success() {
        let stderr = String::from_utf8_lossy(&pull.stderr);
        anyhow::bail!("git pull failed: {}", stderr.trim());
    }

    Ok(())
}

fn ts() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

fn print_banner(device_name: &str, port: u16) {
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║ THE GRID HEADLESS NODE v{:<35} ║", env!("CARGO_PKG_VERSION"));
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║ Device: {:<55}║", device_name);
    println!("║ Agent Port: {:<51}║", port);
    println!("╚═══════════════════════════════════════════════════════════════╝");
}

fn event_line(icon: &str, label: &str, message: impl AsRef<str>) {
    println!("{} {} {:<12} {}", ts(), icon, label, message.as_ref());
}

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

    // Support environment variables
    if let Ok(key) = std::env::var("THEGRID_API_KEY") {
        config.api_key = key;
    }
    if let Ok(name) = std::env::var("THEGRID_DEVICE_NAME") {
        config.device_name = name;
    }

    // Robust CLI Argument Parsing
    let args: Vec<String> = std::env::args().collect();
    let mut skip_update_check = std::env::var("THEGRID_SKIP_UPDATE_CHECK")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut auto_update = std::env::var("THEGRID_AUTO_UPDATE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--api-key" => {
                if i + 1 < args.len() {
                    config.api_key = args[i + 1].trim().to_string();
                    i += 1;
                }
            }
            "--device-name" => {
                if i + 1 < args.len() {
                    config.device_name = args[i + 1].clone();
                    i += 1;
                }
            }
            "--port" | "--agent-port" => {
                if i + 1 < args.len() {
                    if let Ok(p) = args[i + 1].parse::<u16>() {
                        config.agent_port = p;
                        i += 1;
                    }
                }
            }
            "--skip-update-check" => {
                skip_update_check = true;
            }
            "--yes-update" => {
                auto_update = true;
            }
            _ => {
                log::warn!("Unknown argument: {}", args[i]);
            }
        }
        i += 1;
    }

    if config.device_name.is_empty() {
        config.device_name = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "UNKNOWN-NODE".to_string());
    }

    log::info!("Config: device={}, port={}, key_len={}", 
        config.device_name, config.agent_port, config.api_key.len());

    print_banner(&config.device_name, config.agent_port);
    event_line("✓", "BOOT", "Configuration loaded");

    if !skip_update_check {
        match check_latest_release() {
            Ok(Some(release)) => {
                event_line(
                    "⬆",
                    "UPDATE",
                    format!(
                        "New release {} available (current v{})",
                        release.tag_name,
                        env!("CARGO_PKG_VERSION")
                    ),
                );
                event_line("ℹ", "UPDATE", format!("Release: {}", release.html_url));

                let should_update = if auto_update {
                    true
                } else if io::stdin().is_terminal() {
                    prompt_yes_no("Update now? [y/N]: ")
                } else {
                    false
                };

                if should_update {
                    match try_git_update() {
                        Ok(_) => {
                            event_line("✓", "UPDATE", "Repository updated. Restart node to run latest release.");
                            return Ok(());
                        }
                        Err(e) => {
                            event_line("⚠", "UPDATE", format!("Auto-update failed: {}", e));
                            event_line("ℹ", "UPDATE", "You can continue now and update later.");
                        }
                    }
                } else {
                    event_line("⏭", "UPDATE", "Skipped for now");
                }
            }
            Ok(None) => {
                log::debug!("No newer release found.");
            }
            Err(e) => {
                log::debug!("Release check failed (continuing): {}", e);
            }
        }
    }

    // 2. Initialize Runtime
    let (tx, rx) = mpsc::channel::<AppEvent>();
    let runtime = AppRuntime::new(config, tx)?;
    runtime.start_services();

    event_line("▶", "RUNTIME", "Services started. Press Ctrl+C to stop.");

    // Simple loop to handle events
    loop {
        // Drain events
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::SyncRequest { after, response_tx } => {
                    event_line("⇄", "SYNC", format!("Incoming sync request (after={})", after));
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
                    event_line("Δ", "WATCHER", format!("{} ({} paths)", summary, paths.len()));
                    runtime.spawn_incremental_index(paths);
                }
                AppEvent::ClipboardReceived(entry) => {
                    let preview: String = entry.content.chars().take(80).collect();
                    event_line("📋", "CLIPBOARD", format!("from {}: {}", entry.sender, preview));
                    
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
                    event_line("📥", "TRANSFER", format!("{} ({} bytes)", name, size));
                }
                AppEvent::RemoteAiEmbedRequest { text, response_tx } => {
                    event_line("🧠", "AI", format!("Embedding request ({} chars)", text.len()));
                    runtime.handle_remote_ai_embed(text, response_tx);
                }
                AppEvent::RemoteAiSearchRequest { query, k, response_tx } => {
                    event_line("🔎", "AI", format!("Search request k={} query='{}'", k, query));
                    runtime.handle_remote_ai_search(query, k, response_tx);
                }
                AppEvent::RefreshAiServices => {
                    event_line("↻", "AI", "Refreshing AI services");
                    runtime.refresh_ai_services();
                }
                AppEvent::EnableAdb { .. } => {
                    event_line("📱", "ADB", "Enabling ADB over TCP/IP (port 5555)");
                    match std::process::Command::new("adb")
                        .arg("tcpip")
                        .arg("5555")
                        .output() 
                    {
                        Ok(output) => {
                            if output.status.success() {
                                log::info!("ADB daemon restarted on port 5555.");
                            } else {
                                let err = String::from_utf8_lossy(&output.stderr);
                                log::error!("ADB enable failed: {}. Is ADB installed via 'pkg install android-tools'?", err);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to execute 'adb': {}. Ensure android-tools are installed.", e);
                        }
                    }
                }
                AppEvent::EnableRdp { .. } => {
                    log::warn!("EnableRdp received: RDP enablement is not supported on this headless node.");
                }
                AppEvent::AgentPingOk { ip, response, manual } => {
                    if manual {
                        event_line("✓", "PING", format!("{} OK (auth={})", ip, response.authorized));
                    }
                }
                AppEvent::AgentPingFailed { ip, error, manual } => {
                    if manual {
                        event_line("⚠", "PING", format!("{} failed: {}", ip, error));
                    }
                }
                AppEvent::IndexProgress { scanned, total, current, .. } => {
                    if total > 0 {
                        event_line("◷", "INDEX", format!("{}/{} [{}]", scanned, total, current));
                    }
                }
                AppEvent::IndexComplete { files_added, duration_ms, .. } => {
                    event_line("✓", "INDEX", format!("Complete: {} files in {} ms", files_added, duration_ms));
                }
                AppEvent::Status(msg) => {
                    if msg.starts_with("config_update:") {
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

                        let mut cfg = { runtime.config.lock().unwrap().clone() };
                        let mut changed = false;
                        if model.is_some() { cfg.ai_model = model; changed = true; }
                        if url.is_some() { cfg.ai_provider_url = url; changed = true; }
                        
                        if changed {
                            event_line("⚙", "CONFIG", "Received remote config update");
                            let _ = cfg.save();
                            {
                                let mut runtime_cfg = runtime.config.lock().unwrap();
                                *runtime_cfg = cfg;
                            }
                            runtime.refresh_ai_services();
                        }
                    } else if msg.starts_with("index_count:") {
                        log::debug!("Remote index count update: {}", msg);
                    } else {
                        log::debug!("Status: {}", msg);
                    }
                }
                _ => {
                    // Ignore GUI-only events
                    log::trace!("Ignored event: {:?}", event);
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
