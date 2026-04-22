use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use thegrid_core::{AppEvent, Config, Database, FileWatcher};
use thegrid_net::{AgentServer, TailscaleClient};

fn main() -> Result<()> {
    // Quick pre-scan args + env to know if TUI will be active BEFORE logger init.
    // In TUI mode we silence INFO noise — the rolling log panel captures events via emit().
    let raw_args: Vec<String> = std::env::args().collect();
    let pre_plain = raw_args.iter().any(|a| a == "--plain")
        || std::env::var("THEGRID_PLAIN").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let pre_force = raw_args.iter().any(|a| a == "--force-tui")
        || std::env::var("THEGRID_FORCE_TUI").map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false);
    let expect_tui = !pre_plain && (pre_force || (io::stdin().is_terminal() && io::stdout().is_terminal()));

    // In TUI mode silence ALL log output — env_logger writes to stderr which cannot be
    // cleared by the TUI's ANSI escape and would corrupt the layout. All meaningful events
    // are captured through emit() into the rolling log panel instead.
    let log_default = if expect_tui { "off" } else { "info" };
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(log_default)
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
    let mut plain_mode = std::env::var("THEGRID_PLAIN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let mut force_tui = std::env::var("THEGRID_FORCE_TUI")
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
            "--plain" => {
                plain_mode = true;
            }
            "--force-tui" | "--froce-tui" => {
                force_tui = true;
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

    let db = match Database::open(&db_path) {
        Ok(d) => {
            log::info!("Database opened at {:?}", db_path);
            Arc::new(Mutex::new(d))
        }
        Err(e) => {
            log::error!("Failed to open database: {} â€” using in-memory fallback", e);
            Arc::new(Mutex::new(
                Database::open(":memory:").expect("In-memory DB must always work")
            ))
        }
    };

    if !tui_mode {
        print_banner(&config.device_name, config.agent_port);
        println!(
            "  TUI mode : OFF  (stdin_tty={}, stdout_tty={}, plain={}, force={})",
            stdin_tty, stdout_tty, plain_mode, force_tui
        );
        println!("  Tip      : run with --force-tui to enable the interactive interface");
        println!();
    }

    let ui_state = Arc::new(Mutex::new(TuiState::new()));
    if tui_mode {
        emit(&ui_state, tui_mode, "ℹ", "BOOT", "TUI mode active");
    }
    emit(&ui_state, tui_mode, "✓", "BOOT", "Configuration loaded");
    if let Ok(v) = std::env::var(LAST_UPDATE_ENV) {
        if let Some((from, to)) = v.split_once(':') {
            emit(
                &ui_state,
                tui_mode,
                "✓",
                "UPDATE",
                format!("> Version updated {} to {} successfully", from, to),
            );
        }
    }

    if !skip_update_check {
        match check_latest_release() {
            Ok(Some(release)) => {
                emit(
                    &ui_state,
                    tui_mode,
                    "⬆",
                    "UPDATE",
                    format!(
                        "New release {} available (current v{})",
                        release.tag_name,
                        env!("CARGO_PKG_VERSION")
                    ),
                );
                emit(&ui_state, tui_mode, "ℹ", "UPDATE", format!("Release: {}", release.html_url));

                let should_update = if auto_update {
                    true
                } else if io::stdin().is_terminal() {
                    prompt_yes_no("Update now? [y/N]: ")
                } else {
                    false
                };

                if should_update {
                    match try_git_update() {
                        Ok(GitUpdateOutcome::UpToDate { head }) => {
                            emit(&ui_state, tui_mode, "✓", "UPDATE", format!("Version at latest version : {}", head));
                        }
                        Ok(GitUpdateOutcome::Updated { from, to }) => {
                            emit(&ui_state, tui_mode, "✓", "UPDATE", format!("Updated {} -> {}", from, to));
                            emit(&ui_state, tui_mode, "ℹ", "UPDATE", "Restart node to run latest release.");
                            return Ok(());
                        }
                        Err(e) => {
                            emit(&ui_state, tui_mode, "⚠", "UPDATE", format!("Auto-update failed: {}", e));
                            emit(&ui_state, tui_mode, "ℹ", "UPDATE", "You can continue now and update later.");
                        }
                    }
                } else {
                    emit(&ui_state, tui_mode, "⏭", "UPDATE", "Skipped for now");
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

    // 3. Start Agent Server (Port 8080 by default)
    let transfers_dir = config.effective_transfers_dir();
    let config_arc = Arc::new(Mutex::new(config.clone()));
    let mut agent = AgentServer::new(
        config.agent_port,
        config.api_key.clone(),
        transfers_dir.clone(),
        tx.clone(),
        config_arc.clone()
    );

    {
        let cfg = runtime.config.lock().unwrap().clone();
        if cfg.watch_paths.is_empty() {
            emit(
                &ui_state,
                tui_mode,
                "ℹ",
                "INDEX",
                "No watch paths configured. Edit config.json to start indexing.",
            );
        } else {
            let resuming = runtime.db.lock()
                .ok()
                .and_then(|db| db.has_pending_index_tasks().ok())
                .unwrap_or(false);
            emit(
                &ui_state,
                tui_mode,
                "▶",
                "INDEX",
                if resuming {
                    "Resuming unfinished index queue from previous run".to_string()
                } else {
                    format!("Bootstrapping initial index for {} watch path(s)", cfg.watch_paths.len())
                },
            );
            runtime.spawn_index_directories(
                cfg.watch_paths,
                cfg.device_name.clone(),
                cfg.device_name.clone(),
            );
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    let (cmd_tx, cmd_rx) = mpsc::channel::<String>();
    spawn_command_reader(cmd_tx, Arc::clone(&running));
    let mut last_render = Instant::now();

    if tui_mode {
        if let Ok(s) = ui_state.lock() {
            let cfg = runtime.config.lock().unwrap();
            render_tui(&s, &cfg.device_name, cfg.agent_port);
        }
    }

    // Simple loop to handle events
    while running.load(Ordering::Relaxed) {
        while let Ok(cmd_line) = cmd_rx.try_recv() {
            if let Some(effective_cmd) = resolve_history_alias(&cmd_line, &ui_state, tui_mode) {
                execute_command(&effective_cmd, &runtime, &ui_state, tui_mode, &running);
            }
        }

        // Drain events
        while let Ok(event) = rx.try_recv() {
            match event {
                AppEvent::SyncRequest { after, requester_device: _, response_tx } => {
                    log::info!("Incoming sync request (after timestamp: {})", after);
                    if let Ok(guard) = db.lock() {
                        match guard.get_files_after(after) {
                            Ok(results) => {
                                let _ = response_tx.send(thegrid_core::SyncDelta { files: results, tombstones: vec![] });
                            }
                            Err(e) => {
                                log::error!("Failed to query files for sync: {}", e);
                                let _ = response_tx.send(thegrid_core::SyncDelta { files: vec![], tombstones: vec![] });
                            }
                        }
                    }
                }
                AppEvent::FileSystemChanged { changes, summary } => {
                    emit(&ui_state, tui_mode, "Δ", "WATCHER", format!("{} ({} changes)", summary, changes.len()));
                    runtime.spawn_incremental_index(changes);
                }
                AppEvent::ClipboardReceived(entry) => {
                    let preview: String = entry.content.chars().take(80).collect();
                    emit(&ui_state, tui_mode, "⎘", "CLIPBOARD", format!("from {}: {}", entry.sender, preview));
                    
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
                    emit(&ui_state, tui_mode, "↓", "TRANSFER", format!("{} ({} bytes)", name, size));
                }
                AppEvent::RemoteAiEmbedRequest { text, response_tx } => {
                    emit(&ui_state, tui_mode, "◇", "AI", format!("Embedding request ({} chars)", text.len()));
                    runtime.handle_remote_ai_embed(text, response_tx);
                }
                AppEvent::RemoteAiSearchRequest { query, k, response_tx } => {
                    emit(&ui_state, tui_mode, "◈", "AI", format!("Search request k={} query='{}'", k, query));
                    runtime.handle_remote_ai_search(query, k, response_tx);
                }
                AppEvent::RefreshAiServices => {
                    emit(&ui_state, tui_mode, "↻", "AI", "Refreshing AI services");
                    runtime.refresh_ai_services();
                }
                AppEvent::EnableAdb { .. } => {
                    emit(&ui_state, tui_mode, "⇢", "ADB", "Enabling ADB over TCP/IP (port 5555)");
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
                        if let Ok(mut s) = ui_state.lock() {
                            s.ping_ok += 1;
                        }
                        emit(&ui_state, tui_mode, "✓", "PING", format!("{} OK (auth={})", ip, response.authorized));
                    }
                }
                AppEvent::AgentPingFailed { ip, error, manual } => {
                    if manual {
                        if let Ok(mut s) = ui_state.lock() {
                            s.ping_fail += 1;
                        }
                        emit(&ui_state, tui_mode, "⚠", "PING", format!("{} failed: {}", ip, error));
                    }
                }
                AppEvent::DuplicatesFound(groups) => {
                    if groups.is_empty() {
                        emit(&ui_state, tui_mode, "✓", "DUPES", "No duplicate files found in index");
                    } else {
                        let total_files: usize = groups.iter().map(|(_, _, f)| f.len()).sum();
                        let wasted: u64 = groups.iter().map(|(_, size, f)| size * (f.len() as u64 - 1)).sum();
                        emit(&ui_state, tui_mode, "!", "DUPES",
                            format!("{} group(s), {} redundant files, ~{} MB wasted",
                                groups.len(), total_files - groups.len(),
                                wasted / 1_048_576));
                        for (hash, size, files) in groups.iter().take(10) {
                            let paths: Vec<String> = files.iter()
                                .map(|f| f.path.file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| f.path.to_string_lossy().to_string()))
                                .collect();
                            emit(&ui_state, tui_mode, "·", "DUPES",
                                format!("[{}] {}B — {}", &hash[..8], size, paths.join(" | ")));
                        }
                        if groups.len() > 10 {
                            emit(&ui_state, tui_mode, "·", "DUPES",
                                format!("{} more group(s) not shown", groups.len() - 10));
                        }
                    }
                }
                AppEvent::SyncHealthUpdated { device_id, metrics } => {
                    let age_str = metrics.sync_age_secs
                        .map(|a| format!("{}s ago", a))
                        .unwrap_or_else(|| "never".to_string());
                    emit(&ui_state, tui_mode, "⇄", "SYNC",
                        format!("{}: age={} tombs={} fails={}",
                            device_id, age_str, metrics.tombstone_count, metrics.sync_failures));
                    if let Ok(mut s) = ui_state.lock() {
                        s.sync_health.insert(device_id, metrics);
                    }
                }
                AppEvent::IndexProgress { scanned, total, current, .. } => {
                    if total > 0 {
                        emit(&ui_state, tui_mode, "◷", "INDEX", format!("{}/{} [{}]", scanned, total, current));
                    }
                }
                AppEvent::IndexComplete { files_added, duration_ms, .. } => {
                    emit(&ui_state, tui_mode, "✓", "INDEX", format!("Complete: {} files in {} ms", files_added, duration_ms));
                }
                AppEvent::Status(msg) => {
                    if msg.starts_with("db_error:") {
                        emit(&ui_state, tui_mode, "⚠", "DB", &msg["db_error:".len()..]);
                    } else if msg.starts_with("device_ping_ok:") {
                        let parts: Vec<&str> = msg.splitn(3, ':').collect();
                        if parts.len() == 3 {
                            if parts[2] == "true" {
                                emit(&ui_state, tui_mode, "✓", "DEVICE", format!("{} reachable", parts[1]));
                            }
                        }
                    } else if msg.starts_with("device_ping_fail:") {
                        let parts: Vec<&str> = msg.splitn(4, ':').collect();
                        if parts.len() == 4 {
                            if parts[2] == "true" {
                                emit(&ui_state, tui_mode, "⚠", "DEVICE", format!("{} unreachable: {}", parts[1], parts[3]));
                            }
                        }
                    } else if msg.starts_with("config_update:") {
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
                            emit(&ui_state, tui_mode, "⚙", "CONFIG", "Received remote config update");
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

        if tui_mode {
            let force_heartbeat = last_render.elapsed() >= Duration::from_secs(20);
            let mut should_render = force_heartbeat;
            if let Ok(s) = ui_state.lock() {
                should_render = should_render || s.dirty;
            }

            if should_render {
                if let Ok(mut s) = ui_state.lock() {
                    let cfg = runtime.config.lock().unwrap();
                    render_tui(&s, &cfg.device_name, cfg.agent_port);
                    s.dirty = false;
                }
                last_render = Instant::now();
            }
        }

        std::thread::sleep(Duration::from_millis(140));
    }

    Ok(())
}


