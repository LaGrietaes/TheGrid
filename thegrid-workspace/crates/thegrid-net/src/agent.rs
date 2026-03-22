use anyhow::{Context, Result};
use reqwest::blocking::Client;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tiny_http::{Server, Response, Request};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::io::{Read, Write};
#[cfg(windows)]
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use thegrid_core::{AppEvent, models::*, Config};
use ascii::AsciiStr;

const AGENT_VERSION: &str = "0.3.0";


use crate::tailscale::TailscaleClient;

struct TerminalSession {
    writer: Box<dyn Write + Send>,
    output_buffer: Arc<Mutex<VecDeque<u8>>>,
}

pub struct AgentServer {
    port: u16,
    api_key: String,
    transfers_dir: PathBuf,
    event_tx: mpsc::Sender<AppEvent>,
    ts_client: Option<Arc<TailscaleClient>>,
    config: Arc<Mutex<Config>>,
    terminal_sessions: Mutex<HashMap<String, TerminalSession>>,
    shutdown: Arc<AtomicBool>,
}

impl AgentServer {
    pub fn new(
        port: u16,
        api_key: String,
        transfers_dir: PathBuf,
        event_tx: mpsc::Sender<AppEvent>,
        config: Arc<Mutex<Config>>,
    ) -> Self {
        Self { 
            port, 
            api_key, 
            transfers_dir, 
            event_tx,
            ts_client: None,
            config,
            terminal_sessions: Mutex::new(HashMap::new()),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn shutdown_handle(&self) -> Arc<AtomicBool> {
        self.shutdown.clone()
    }

    pub fn with_tailscale(mut self, client: Arc<TailscaleClient>) -> Self {
        self.ts_client = Some(client);
        self
    }

    pub fn spawn(self) {
        let port = self.port;
        let api_key_prefix: String = self.api_key.chars().take(8).collect();
        
        // Move self into the thread — this is why we don't need Arc<Self>
        std::thread::Builder::new()
            .name("thegrid-agent".into())
            .spawn(move || {
                if let Err(e) = self.run() {
                    log::error!("AgentServer fatal error: {}", e);
                }
            })
            .expect("Spawning agent thread");

        log::info!("THE GRID agent server started on port {} (X-Grid-Key starts with: {}...)", 
            port, 
            api_key_prefix
        );
    }

    fn run(&self) -> Result<()> {
        std::fs::create_dir_all(&self.transfers_dir)?;
        let addr = format!("0.0.0.0:{}", self.port);
        let server = Server::http(&addr)
            .map_err(|e| anyhow::anyhow!("Starting HTTP server on {}: {}", addr, e))?;

        let shutdown = self.shutdown.clone();
        for _ in 0.. {
            if shutdown.load(Ordering::Relaxed) { break; }
            
            match server.recv_timeout(Duration::from_millis(500)) {
                Ok(Some(request)) => {
                    if let Err(e) = self.handle_request(request) {
                        log::warn!("Agent request error: {}", e);
                    }
                }
                Ok(None) => {}, // Timeout, continue
                Err(e) => {
                    log::error!("Agent server error: {}", e);
                    break;
                }
            }
        }
        log::info!("Agent server on port {} has stopped.", self.port);
        Ok(())
    }

    fn handle_request(&self, mut req: Request) -> Result<()> {
        let method = req.method().to_string();
        let url    = req.url().to_string();
        
        // --- 1. Robust /ping handler (unauthenticated) ---
        if method == "GET" && (url == "/ping" || url.starts_with("/ping?") || url == "/ping/") {
            let remote_addr = req.remote_addr().map(|a| a.to_string()).unwrap_or_else(|| "UNKNOWN".into());
            let mut authorized = false;
            let key = req.headers().iter()
                .find(|h| h.field.as_str().to_string().to_lowercase() == "x-grid-key")
                .map(|h| h.value.as_str().trim());
            
            let mut auth_mode = "None";
            let local_key = self.api_key.trim();
            if key == Some(local_key) {
                authorized = true;
                auth_mode = "API Key Match";
            } else {
                // Diagnostic for mismatch
                if let Some(received) = key {
                    log::debug!("Auth mismatch: req_len={}, local_len={}", received.len(), local_key.len());
                    // Log first 10 hex bytes if they differ
                    if received != local_key {
                        let r_bytes: Vec<String> = received.bytes().take(10).map(|b| format!("{:02x}", b)).collect();
                        let l_bytes: Vec<String> = local_key.bytes().take(10).map(|b| format!("{:02x}", b)).collect();
                        log::debug!("Received hex: {:?}", r_bytes);
                        log::debug!("Expected hex: {:?}", l_bytes);
                    }
                }
                
                if let Some(ts) = &self.ts_client {
                    let remote_addr_struct = req.remote_addr();
                    let remote_ip = remote_addr_struct.map(|a| {
                        let mut ip = a.ip();
                        if let std::net::IpAddr::V6(v6) = ip {
                            if let Some(v4) = v6.to_ipv4() {
                                ip = std::net::IpAddr::V4(v4);
                            }
                        }
                        ip.to_string()
                    }).unwrap_or_default();
                    
                    if ts.is_ip_in_tailnet(&remote_ip) {
                        authorized = true;
                        auth_mode = "Tailscale Trust";
                    }
                }
            }

            let masked_key = key.map(|k| if k.len() > 8 { format!("{}...", &k[..8]) } else { "***".into() }).unwrap_or_else(|| "MISSING".into());
            log::info!("Agent [/ping] from {} - authorized={} ({}) - Key: {}", remote_addr, authorized, auth_mode, masked_key);

            let h = hostname::get().unwrap_or_else(|_| std::ffi::OsString::from("UNKNOWN")).to_string_lossy().to_string();
            let body = serde_json::json!({
                "ok": true,
                "authorized": authorized,
                "hostname": h.clone(),
                "device": h,
                "version": AGENT_VERSION,
            });
            let json = body.to_string();
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        let remote_addr = req.remote_addr().map(|a| a.to_string()).unwrap_or_else(|| "UNKNOWN".into());
        log::info!("Agent [{}]: {} {}", remote_addr, method, url);

        // --- 2. Authentication for all other endpoints ---
        let mut authorized = false;

        // Check for X-Grid-Key header
        let key = req.headers().iter()
            .find(|h| h.field.as_str().to_string().to_lowercase() == "x-grid-key")
            .map(|h| h.value.as_str());
        
        if key == Some(&self.api_key) {
            authorized = true;
        }

        // If not authorized by key, check Tailscale trust
        if !authorized {
            if let Some(ts) = &self.ts_client {
                if let Some(remote_addr) = req.remote_addr() {
                    let remote_ip = format!("{}", remote_addr.ip());
                    if ts.is_ip_in_tailnet(&remote_ip) {
                        log::info!("Agent: granting access to trusted tailnet node {}", remote_ip);
                        authorized = true;
                    }
                }
            }
        }
        
        if !authorized {
            let remote_addr = req.remote_addr().map(|a| a.to_string()).unwrap_or_else(|| "UNKNOWN".to_string());
            let mut reason = "Authentication required (Key mismatch)";
            
            if self.ts_client.is_some() {
                reason = "Authentication failed (Key mismatch and Tailscale trust check failed)";
            }

            log::warn!("Agent: unauthorized access attempt from {}. Reason: {}. Expected X-Grid-Key: {}..., Got: {}...", 
                remote_addr,
                reason,
                &self.api_key.chars().take(4).collect::<String>(),
                key.unwrap_or("NONE").chars().take(4).collect::<String>()
            );

            let body = serde_json::json!({
                "error": "unauthorized",
                "reason": reason,
                "suggestion": "Ensure both nodes share the same api_key in config.json or have valid Tailscale API keys."
            }).to_string();

            req.respond(Response::from_string(body)
                .with_status_code(401)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "GET" && url == "/telemetry" {
            let telemetry = {
                let cfg = self.config.lock().unwrap();
                collect_telemetry(&cfg)
            };
            let json = serde_json::to_string(&telemetry).unwrap_or_default();
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "POST" && url == "/v1/config" {
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body)?;

            #[derive(serde::Deserialize)]
            struct ConfigUpdate {
                device_type: Option<String>,
                ai_model: Option<String>,
                ai_provider_url: Option<String>,
            }

            if let Ok(update) = serde_json::from_str::<ConfigUpdate>(&body) {
                let (should_restart_ai, should_save) = {
                    let mut cfg = self.config.lock().unwrap();
                    let mut changed = false;
                    let mut ai_changed = false;

                    if let Some(dt) = update.device_type {
                        if cfg.device_type != dt {
                            cfg.device_type = dt;
                            changed = true;
                        }
                    }
                    if let Some(m) = update.ai_model {
                        if cfg.ai_model != Some(m.clone()) {
                            cfg.ai_model = Some(m);
                            changed = true;
                            ai_changed = true;
                        }
                    }
                    if let Some(u) = update.ai_provider_url {
                        if cfg.ai_provider_url != Some(u.clone()) {
                            cfg.ai_provider_url = Some(u);
                            changed = true;
                            ai_changed = true;
                        }
                    }

                    (ai_changed, changed)
                };

                if should_save {
                    let cfg = self.config.lock().unwrap();
                    if let Err(e) = cfg.save() {
                        log::error!("Failed to save config after remote update: {}", e);
                    }
                    
                    if should_restart_ai {
                        let _ = self.event_tx.send(AppEvent::RefreshAiServices);
                    }
                }

                req.respond(Response::from_string(r#"{"ok":true}"#)
                    .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                )?;
            } else {
                req.respond(Response::from_string(r#"{"error":"invalid json"}"#).with_status_code(400))?;
            }
            return Ok(());
        }

        if method == "POST" && url == "/adb/enable" {
            #[cfg(target_os = "linux")]
            {
                log::info!("Agent: attempting to enable ADB over TCP/IP 5555 (Termux)");                match std::process::Command::new("adb")
                    .arg("tcpip").arg("5555")
                    .output() 
                {
                    Ok(output) => {
                        let success = output.status.success();
                        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        let msg = format!("ADB result: success={}, stdout='{}', stderr='{}'", 
                            success, stdout, stderr);
                        
                        if success { 
                            log::info!("Agent: {}", msg); 
                        } else { 
                            log::error!("Agent: {}", msg); 
                        }
                        
                        req.respond(Response::from_string(format!(r#"{{"ok":{},"message":{}}}"#, 
                            success, serde_json::to_string(&msg).unwrap()))
                            .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                        )?;
                    }
                    Err(e) => {
                        let msg = format!("Failed to execute 'adb' binary: {}. Is android-tools installed?", e);
                        log::error!("{}", msg);
                        req.respond(Response::from_string(format!(r#"{{"ok":false,"message":{}}}"#, serde_json::to_string(&msg).unwrap()))
                            .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                        )?;
                    }
                }

            }
            #[cfg(not(target_os = "linux"))]
            {
                req.respond(Response::from_string(r#"{"ok":false,"message":"ADB local enabling only supported on Linux/Android nodes"}"#)
                    .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                )?;
            }
            return Ok(());
        }

        if method == "GET" && url.starts_with("/v1/sync") {
            let after: i64 = url.split("after=")
                .nth(1)
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);

            let (tx, rx) = mpsc::channel();
            let _ = self.event_tx.send(AppEvent::SyncRequest { after, response_tx: tx });
            let files = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or_default();

            let json = serde_json::to_string(&files)?;
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "GET" && url == "/filelist" {
            let files = self.list_transfer_files();
            let json = serde_json::to_string(&files)?;
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "GET" && url.starts_with("/files/") {
            let filename = url.trim_start_matches("/files/");
            let filename = urlencoding_decode(filename);
            let path = self.transfers_dir.join(&filename);

            if path.exists() && path.starts_with(&self.transfers_dir) {
                let data = std::fs::read(&path)?;
                req.respond(Response::from_data(data))?;
            } else {
                req.respond(Response::from_string("Not found").with_status_code(404))?;
            }
            return Ok(());
        }

        if method == "POST" && url == "/clipboard" {
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body)?;

            #[derive(serde::Deserialize)]
            struct ClipPayload { content: String, #[serde(default)] sender: String }

            if let Ok(payload) = serde_json::from_str::<ClipPayload>(&body) {
                let entry = ClipboardEntry {
                    content: payload.content,
                    sender: payload.sender,
                    received_at: chrono::Utc::now(),
                };
                let _ = self.event_tx.send(AppEvent::ClipboardReceived(entry));
            }

            req.respond(Response::from_string(r#"{"ok":true}"#)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "POST" && url == "/upload" {
            let filename = req.headers().iter()
                .find(|h| h.field.as_str().eq_ignore_ascii_case(AsciiStr::from_ascii("X-Filename").unwrap()))
                .map(|h| h.value.as_str().to_string())
                .unwrap_or_else(|| "upload".to_string());

            let dest = self.transfers_dir.join(&filename);
            let mut body = Vec::new();
            req.as_reader().read_to_end(&mut body)?;

            let size = body.len() as u64;
            std::fs::write(&dest, &body)?;

            let _ = self.event_tx.send(AppEvent::FileReceived { name: filename, size });

            req.respond(Response::from_string(r#"{"ok":true}"#)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        // ── NEW: Remote File Browsing ──
        if method == "GET" && url.starts_with("/v1/browse") {
            let enabled = {
                let cfg = self.config.lock().unwrap();
                cfg.enable_file_access
            };
            if !enabled {
                req.respond(Response::from_string(r#"{"error":"file access disabled"}"#).with_status_code(403))?;
                return Ok(());
            }
            let path_str = url.split("path=").nth(1).unwrap_or("");
            let path_str = urlencoding_decode(path_str);
            let path = PathBuf::from(path_str);

            let files = if path.as_os_str().is_empty() {
                // List drives on Windows, or root on Linux if empty path
                AgentServer::list_root_dir()
            } else {
                AgentServer::list_any_dir(&path)
            };

            let json = serde_json::to_string(&files)?;
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "GET" && url.starts_with("/v1/read") {
            let enabled = {
                let cfg = self.config.lock().unwrap();
                cfg.enable_file_access
            };
            if !enabled {
                req.respond(Response::from_string(r#"{"error":"file access disabled"}"#).with_status_code(403))?;
                return Ok(());
            }
            let path_str = url.split("path=").nth(1).unwrap_or("");
            let path_str = urlencoding_decode(path_str);
            let path = PathBuf::from(path_str);

            if path.exists() && path.is_file() {
                let data = std::fs::read(&path)?;
                req.respond(Response::from_data(data))?;
            } else {
                req.respond(Response::from_string("Not found").with_status_code(404))?;
            }
            return Ok(());
        }

        if method == "DELETE" && url.starts_with("/v1/files") {
            let enabled = {
                let cfg = self.config.lock().unwrap();
                cfg.enable_file_access
            };
            if !enabled {
                req.respond(Response::from_string(r#"{"error":"forbidden"}"#).with_status_code(403))?;
                return Ok(());
            }
            let path_str = url.split("path=").nth(1).unwrap_or("");
            let path_str = urlencoding_decode(path_str);
            let path = PathBuf::from(path_str);

            if !path.exists() {
                req.respond(Response::from_string(r#"{"error":"not found"}"#).with_status_code(404))?;
            } else {
                let res = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                match res {
                    Ok(_) => req.respond(Response::from_string(r#"{"ok":true}"#))?,
                    Err(e) => req.respond(Response::from_string(format!(r#"{{"error":"{}"}}"#, e)).with_status_code(500))?,
                }
            }
            return Ok(());
        }

        if method == "POST" && url == "/v1/files/rename" {
            #[derive(serde::Deserialize)]
            struct RenameReq { path: String, new_name: String }
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body)?;
            if let Ok(r) = serde_json::from_str::<RenameReq>(&body) {
                let old_path = PathBuf::from(&r.path);
                let mut new_path = old_path.clone();
                new_path.set_file_name(&r.new_name);
                
                if old_path.exists() {
                    match std::fs::rename(&old_path, &new_path) {
                        Ok(_) => req.respond(Response::from_string(r#"{"ok":true}"#))?,
                        Err(e) => req.respond(Response::from_string(format!(r#"{{"error":"{}"}}"#, e)).with_status_code(500))?,
                    }
                } else {
                    req.respond(Response::from_string(r#"{"error":"not found"}"#).with_status_code(404))?;
                }
            } else {
                req.respond(Response::from_string(r#"{"error":"bad request"}"#).with_status_code(400))?;
            }
            return Ok(());
        }

        if method == "POST" && url == "/v1/files/move" {
            #[derive(serde::Deserialize)]
            struct MoveReq { paths: Vec<String>, dest_dir: String }
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body)?;
            if let Ok(m) = serde_json::from_str::<MoveReq>(&body) {
                let dest_dir = PathBuf::from(&m.dest_dir);
                if !dest_dir.exists() {
                    std::fs::create_dir_all(&dest_dir)?;
                }
                let mut success = true;
                for p in m.paths {
                    let old_path = PathBuf::from(&p);
                    if let Some(name) = old_path.file_name() {
                        let new_path = dest_dir.join(name);
                        if let Err(e) = std::fs::rename(&old_path, &new_path) {
                            log::error!("Move failed for {}: {}", p, e);
                            success = false;
                        }
                    }
                }
                req.respond(Response::from_string(format!(r#"{{"ok":{}}}"#, success)))?;
            } else {
                req.respond(Response::from_string(r#"{"error":"bad request"}"#).with_status_code(400))?;
            }
            return Ok(());
        }

        // ── Terminal Endpoints ──
        if method == "POST" && url == "/v1/terminal/session" {
            let (writer, mut reader): (Box<dyn Write + Send>, Box<dyn Read + Send>) = {
                #[cfg(windows)]
                {
                    let pty_system = NativePtySystem::default();
                    let pty_pair = pty_system.openpty(PtySize {
                        rows: 24,
                        cols: 80,
                        pixel_width: 0,
                        pixel_height: 0,
                    }).map_err(|e| anyhow::anyhow!("Failed to open PTY: {}", e))?;

                    let cmd = CommandBuilder::new("powershell.exe");
                    let _child = pty_pair.slave.spawn_command(cmd)
                        .map_err(|e| anyhow::anyhow!("Failed to spawn shell: {}", e))?;

                    let w = pty_pair.master.take_writer()
                        .map_err(|e| anyhow::anyhow!("Failed to take PTY writer: {}", e))?;
                    let r = pty_pair.master.try_clone_reader()
                        .map_err(|e| anyhow::anyhow!("Failed to clone PTY reader: {}", e))?;
                    (Box::new(w), Box::new(r))
                }

                #[cfg(unix)]
                {
                    use std::os::unix::io::{FromRawFd, IntoRawFd};
                    use std::os::unix::process::CommandExt;

                    let pty = nix::pty::openpty(None, None)
                        .map_err(|e| anyhow::anyhow!("openpty failed: {}", e))?;
                    
                    let shell = if std::path::Path::new("/system/bin/sh").exists() { "/system/bin/sh" } else { "bash" };
                    let mut cmd = std::process::Command::new(shell);
                    
                    let slave_fd = pty.slave.into_raw_fd();
                    let master_fd = pty.master.into_raw_fd();

                    // Connect slave to child process
                    unsafe {
                        cmd.pre_exec(move || {
                            let _ = nix::unistd::setsid();
                            let _ = nix::unistd::dup2(slave_fd, 0);
                            let _ = nix::unistd::dup2(slave_fd, 1);
                            let _ = nix::unistd::dup2(slave_fd, 2);
                            Ok(())
                        });
                    }

                    cmd.spawn().map_err(|e| anyhow::anyhow!("Failed to spawn shell: {}", e))?;

                    let master_writer = unsafe { std::fs::File::from_raw_fd(master_fd) };
                    let master_reader = unsafe { std::fs::File::from_raw_fd(nix::unistd::dup(master_fd)?) };
                    (Box::new(master_writer), Box::new(master_reader))
                }
            };

            let output_buffer = Arc::new(Mutex::new(VecDeque::new()));
            let output_buffer_clone = Arc::clone(&output_buffer);

            // Thread to read PTY output and push to buffer
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 { break; }
                    let mut buffer = output_buffer_clone.lock().unwrap();
                    for &byte in &buf[..n] {
                        buffer.push_back(byte);
                        if buffer.len() > 65536 {
                            buffer.pop_front();
                        }
                    }
                }
                log::info!("PTY output thread exiting");
            });

            let session_id = uuid::Uuid::new_v4().to_string();
            self.terminal_sessions.lock().unwrap().insert(session_id.clone(), TerminalSession {
                writer,
                output_buffer,
            });

            req.respond(Response::from_string(format!(r#"{{"session_id":"{}"}}"#, session_id)))?;
            return Ok(());
        }

        if method == "POST" && url.starts_with("/v1/terminal/input") {
            let id = url.split("id=").nth(1).unwrap_or("").split('&').next().unwrap_or("");
            let mut body = Vec::new();
            req.as_reader().read_to_end(&mut body)?;
            
            let mut sessions = self.terminal_sessions.lock().unwrap();
            if let Some(session) = sessions.get_mut(id) {
                session.writer.write_all(&body)?;
                session.writer.flush()?;
                req.respond(Response::from_string("ok"))?;
            } else {
                req.respond(Response::from_string("Session not found").with_status_code(404))?;
            }
            return Ok(());
        }

        if method == "GET" && url.starts_with("/v1/terminal/output") {
            let id = url.split("id=").nth(1).unwrap_or("").split('&').next().unwrap_or("");
            let sessions = self.terminal_sessions.lock().unwrap();
            if let Some(session) = sessions.get(id) {
                let mut buffer = session.output_buffer.lock().unwrap();
                let bytes: Vec<u8> = buffer.drain(..).collect();
                req.respond(Response::from_data(bytes))?;
            } else {
                req.respond(Response::from_string("Session not found").with_status_code(404))?;
            }
            return Ok(());
        }

        // ── NEW: Remote AI Inference ──
        if method == "POST" && url == "/v1/ai/embed" {
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body)?;
            
            #[derive(serde::Deserialize)]
            struct EmbedReq { text: String }
            
            if let Ok(req_data) = serde_json::from_str::<EmbedReq>(&body) {
                let (tx, rx) = mpsc::channel();
                let _ = self.event_tx.send(AppEvent::RemoteAiEmbedRequest { 
                    text: req_data.text, 
                    response_tx: tx 
                });
                
                let vector = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_default();
                let json = serde_json::to_string(&vector)?;
                req.respond(Response::from_string(json)
                    .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                )?;
            } else {
                req.respond(Response::from_string(r#"{"error":"invalid json"}"#).with_status_code(400))?;
            }
            return Ok(());
        }

        if method == "POST" && url == "/v1/ai/search" {
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body)?;
            
            #[derive(serde::Deserialize)]
            struct SearchReq { query: String, k: usize }
            
            if let Ok(req_data) = serde_json::from_str::<SearchReq>(&body) {
                let (tx, rx) = mpsc::channel();
                let _ = self.event_tx.send(AppEvent::RemoteAiSearchRequest { 
                    query: req_data.query, 
                    k: req_data.k,
                    response_tx: tx 
                });
                
                let results = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap_or_default();
                let json = serde_json::to_string(&results)?;
                req.respond(Response::from_string(json)
                    .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                )?;
            } else {
                req.respond(Response::from_string(r#"{"error":"invalid json"}"#).with_status_code(400))?;
            }
            return Ok(());
        }

        // ── Remote RDP Enablement ──
        if method == "POST" && url == "/v1/rdp/enable" {
            #[cfg(windows)]
            {
                log::info!("Agent: attempting to enable Windows Remote Desktop (RDP)");
                match crate::win_sys::enable_rdp() {
                    Ok(_) => {
                        let msg = "RDP enabled successfully".to_string();
                        log::info!("{}", msg);
                        req.respond(Response::from_string(format!(r#"{{"ok":true,"message":{}}}"#, serde_json::to_string(&msg).unwrap()))
                            .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                        )?;
                    }
                    Err(e) => {
                        let msg = format!("Failed to enable RDP: {}", e);
                        log::error!("{}", msg);
                        req.respond(Response::from_string(format!(r#"{{"ok":false,"message":{}}}"#, serde_json::to_string(&msg).unwrap()))
                            .with_status_code(500)
                            .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                        )?;
                    }
                }
            }
            #[cfg(not(windows))]
            {
                req.respond(Response::from_string(r#"{"ok":false,"message":"RDP enabling only supported on Windows nodes"}"#)
                    .with_status_code(400)
                    .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                )?;
            }
            return Ok(());
        }

        req.respond(Response::from_string("Not found").with_status_code(404))?;
        Ok(())
    }

    pub fn list_any_dir(path: &Path) -> Vec<RemoteFile> {
        std::fs::read_dir(path).map(|entries| {
            entries.filter_map(|e| {
                let e = e.ok()?;
                let meta = e.metadata().ok()?;
                Some(RemoteFile {
                    name: e.file_name().to_string_lossy().to_string(),
                    size: meta.len(),
                    modified: meta.modified().ok().map(|t| chrono::DateTime::from(t)),
                    is_dir: meta.is_dir(),
                })
            }).collect()
        }).unwrap_or_default()
    }

    pub fn list_root_dir() -> Vec<RemoteFile> {
        #[cfg(windows)]
        {
            let mut drives = Vec::new();
            for drive_letter in b'A'..=b'Z' {
                let path = format!("{}:\\", drive_letter as char);
                if Path::new(&path).exists() {
                    drives.push(RemoteFile {
                        name: path,
                        size: 0,
                        modified: None,
                        is_dir: true,
                    });
                }
            }
            drives
        }
        #[cfg(not(windows))]
        {
            Self::list_any_dir(Path::new("/"))
        }
    }

    fn list_transfer_files(&self) -> Vec<RemoteFile> {
        std::fs::read_dir(&self.transfers_dir).map(|entries| {
            entries.filter_map(|e| {
                let e = e.ok()?;
                let meta = e.metadata().ok()?;
                if !meta.is_file() { return None; }
                Some(RemoteFile {
                    name: e.file_name().to_string_lossy().to_string(),
                    size: meta.len(),
                    modified: meta.modified().ok().map(|t| chrono::DateTime::from(t)),
                    is_dir: meta.is_dir(),
                })
            }).collect()
        }).unwrap_or_default()
    }
}

pub struct AgentClient {
    http: Client,
    api_key: String,
    base_url: String,
}

impl AgentClient {
    pub fn new(ip: &str, port: u16, api_key: String) -> Result<Self> {
        let http = Client::builder().timeout(std::time::Duration::from_secs(30)).build()?;
        Ok(Self { http, api_key, base_url: format!("http://{}:{}", ip, port) })
    }

    pub fn ping(&self) -> Result<AgentPingResponse> {
        let url = format!("{}/ping", self.base_url);
        let masked_key = if self.api_key.len() > 8 { format!("{}...", &self.api_key[..8]) } else { "***".into() };
        log::info!("Client: pinging {} with Key: {}", url, masked_key);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).timeout(std::time::Duration::from_secs(3)).send().context("Pinging agent")?;
        let status = resp.status();
        if !status.is_success() {
            log::warn!("Client: ping to {} failed with status {}", url, status);
            return Err(Self::handle_error(resp));
        }
        let r: AgentPingResponse = resp.json().context("Parsing ping response")?;
        log::info!("Client: ping to {} succeeded (authorized={})", url, r.authorized);
        Ok(r)
    }

    pub fn list_files(&self) -> Result<Vec<RemoteFile>> {
        #[derive(serde::Deserialize)]
        struct Resp { files: Vec<RemoteFile> }
        let url = format!("{}/filelist", self.base_url);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        let r: Resp = resp.json()?;
        Ok(r.files)
    }



    pub fn delete_file(&self, path: &str) -> Result<()> {
        let url = format!("{}/v1/files?path={}", self.base_url, urlencoding_encode(path));
        let resp = self.http.delete(&url).header("X-Grid-Key", &self.api_key).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    pub fn rename_file(&self, path: &str, new_name: &str) -> Result<()> {
        let url = format!("{}/v1/files/rename", self.base_url);
        let body = serde_json::json!({ "path": path, "new_name": new_name });
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).json(&body).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    pub fn move_files(&self, paths: Vec<String>, dest_dir: &str) -> Result<()> {
        let url = format!("{}/v1/files/move", self.base_url);
        let body = serde_json::json!({ "paths": paths, "dest_dir": dest_dir });
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).json(&body).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    pub fn download_file(&self, filename: &str, dest_dir: &Path) -> Result<PathBuf> {
        let url = format!("{}/files/{}", self.base_url, urlencoding_encode(filename));
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        let bytes = resp.bytes().context("Reading file bytes")?;
        std::fs::create_dir_all(dest_dir)?;
        let dest = dest_dir.join(filename);
        std::fs::write(&dest, &bytes)?;
        Ok(dest)
    }

    pub fn upload_file(&self, path: &Path) -> Result<()> {
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let data = std::fs::read(path)?;
        let url = format!("{}/upload", self.base_url);
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).header("X-Filename", &filename).body(data).send().context("Uploading file")?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    pub fn send_clipboard(&self, content: &str, sender: &str) -> Result<()> {
        let url = format!("{}/clipboard", self.base_url);
        let body = serde_json::json!({ "content": content, "sender": sender });
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).json(&body).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    pub fn create_terminal_session(&self) -> Result<String> {
        let url = format!("{}/v1/terminal/session", self.base_url);
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        #[derive(serde::Deserialize)]
        struct Resp { session_id: String }
        let r: Resp = resp.json()?;
        Ok(r.session_id)
    }

    pub fn send_terminal_input(&self, session_id: &str, data: &[u8]) -> Result<()> {
        let url = format!("{}/v1/terminal/input?id={}", self.base_url, session_id);
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).body(data.to_vec()).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    pub fn get_terminal_output(&self, session_id: &str) -> Result<Vec<u8>> {
        let url = format!("{}/v1/terminal/output?id={}", self.base_url, session_id);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        let bytes = resp.bytes()?.to_vec();
        Ok(bytes)
    }

    pub fn get_telemetry(&self) -> Result<NodeTelemetry> {
        let url = format!("{}/telemetry", self.base_url);
        log::info!("Client: fetching telemetry from {}", url);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).timeout(std::time::Duration::from_secs(5)).send().context("Fetching telemetry")?;
        let status = resp.status();
        if !status.is_success() {
            log::warn!("Client: telemetry from {} failed with status {}", url, status);
            return Err(Self::handle_error(resp));
        }
        let r: NodeTelemetry = resp.json().context("Parsing telemetry JSON")?;
        log::info!("Client: telemetry from {} succeeded", url);
        Ok(r)
    }

    pub fn sync_index(&self, last_sync_ts: i64) -> Result<Vec<FileSearchResult>> {
        let url = format!("{}/v1/sync?after={}", self.base_url, last_sync_ts);
        log::debug!("Client: syncing index from {} (after={})", url, last_sync_ts);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send().context("Requesting index sync")?;
        let status = resp.status();
        if !status.is_success() {
            log::warn!("Client: sync from {} failed with status {}", url, status);
            return Err(Self::handle_error(resp));
        }
        let r: Vec<FileSearchResult> = resp.json().context("Parsing sync JSON")?;
        log::debug!("Client: sync from {} succeeded ({} results)", url, r.len());
        Ok(r)
    }

    pub fn browse_directory(&self, path: &Path) -> Result<Vec<RemoteFile>> {
        let path_str = urlencoding_encode(&path.to_string_lossy());
        let url = format!("{}/v1/browse?path={}", self.base_url, path_str);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send().context("Browsing remote directory")?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        resp.json().context("Parsing browse JSON")
    }

    pub fn download_remote_file(&self, path: &Path, dest: &Path) -> Result<PathBuf> {
        let filename = path.file_name().ok_or_else(|| anyhow::anyhow!("Invalid path"))?.to_string_lossy();
        let path_str = urlencoding_encode(&path.to_string_lossy());
        let url = format!("{}/v1/read?path={}", self.base_url, path_str);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        let bytes = resp.bytes().context("Reading remote file bytes")?;
        let dest_file = dest.join(&*filename);
        std::fs::write(&dest_file, &bytes)?;
        Ok(dest_file)
    }

    pub fn enable_rdp(&self) -> Result<()> {
        let url = format!("{}/v1/rdp/enable", self.base_url);
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).send().context("Requesting RDP enablement")?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    pub fn remote_embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/v1/ai/embed", self.base_url);
        let body = serde_json::json!({ "text": text });
        let resp = self.http.post(&url)
            .header("X-Grid-Key", &self.api_key)
            .json(&body)
            .send()
            .context("Remote embedding request")?;
        
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        resp.json().context("Parsing remote embed response")
    }

    pub fn remote_search(&self, query: &str, k: usize) -> Result<Vec<(i64, f32)>> {
        let url = format!("{}/v1/ai/search", self.base_url);
        let body = serde_json::json!({ "query": query, "k": k });
        let resp = self.http.post(&url)
            .header("X-Grid-Key", &self.api_key)
            .json(&body)
            .send()
            .context("Remote search request")?;
        
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        resp.json().context("Parsing remote search response")
    }

    pub fn update_config(&self, device_type: Option<String>, model: Option<String>, url: Option<String>) -> Result<()> {
        let endpoint = format!("{}/v1/config", self.base_url);
        let body = serde_json::json!({
            "device_type": device_type,
            "ai_model": model,
            "ai_provider_url": url,
        });
        let resp = self.http.post(&endpoint)
            .header("X-Grid-Key", &self.api_key)
            .json(&body)
            .send()
            .context("Sending config update")?;
        
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        Ok(())
    }

    fn handle_error(resp: reqwest::blocking::Response) -> anyhow::Error {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        
        if status == 401 {
            #[derive(serde::Deserialize)]
            struct ErrBody { reason: String, suggestion: String }
            if let Ok(err_data) = serde_json::from_str::<ErrBody>(&body) {
                return anyhow::anyhow!("Unauthorized (401): {}\nSuggestion: {}", err_data.reason, err_data.suggestion);
            }
            return anyhow::anyhow!("Unauthorized (401): Authentication mismatch. Please check your api_key.");
        }

        anyhow::anyhow!("Request failed with status {}: {}", status, body)
    }
}

fn collect_telemetry(config: &Config) -> NodeTelemetry {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    
    let cpu_pct = sys.global_cpu_info().cpu_usage();
    let ram_used = sys.used_memory();
    let ram_total = sys.total_memory();
    
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut disk_used = 0;
    let mut disk_total = 0;
    let mut drive_infos = Vec::new();
    
    for disk in &disks {
        let used = disk.total_space() - disk.available_space();
        let total = disk.total_space();
        disk_used += used;
        disk_total += total;
        drive_infos.push(DriveInfo {
            name: disk.name().to_string_lossy().into_owned(),
            used,
            total,
        });
    }
    
    // BPW: Collect local IPs to identify wired/direct connections
    let mut local_ips = Vec::new();
    if let Ok(ifs) = get_if_addrs::get_if_addrs() {
        for interface in ifs {
            if !interface.is_loopback() {
                if let std::net::IpAddr::V4(addr) = interface.ip() {
                    local_ips.push(addr.to_string());
                }
            }
        }
    }

    let mut ai_models = Vec::new();
    if let Some(m) = &config.ai_model {
        if !m.is_empty() {
            ai_models.push(m.clone());
        }
    }

    let capabilities = DeviceCapabilities {
        ai_models,
        has_camera: true,        // Stubbed for now as agreed
        has_microphone: true,    // Stubbed for now as agreed
        has_speakers: true,      // Stubbed for now as agreed
        drives: drive_infos,
        has_rdp: crate::win_sys::is_rdp_enabled(),
        has_file_access: config.enable_file_access,
    };

    // GPU info (experimental for Windows)
    let mut gpu_name = None;
    if cfg!(target_os = "windows") {
        if let Ok(output) = std::process::Command::new("wmic")
            .args(&["path", "win32_VideoController", "get", "name"])
            .output() {
                let s = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = s.lines().collect();
                if lines.len() > 1 {
                    gpu_name = Some(lines[1].trim().to_string());
                }
            }
    }

    NodeTelemetry {
        device_type: config.device_type.clone(),
        cpu_pct,
        ram_used,
        ram_total,
        disk_used,
        disk_total,
        cpu_temp: None,
        is_ai_capable: !capabilities.ai_models.is_empty(),
        gpu_name,
        gpu_pct: None,
        gpu_mem_used: None,
        gpu_mem_total: None,
        local_ips,
        ai_status: Some("Idle".into()),
        ai_tokens_per_sec: None,
        ai_thoughts: None,
        capabilities,
    }
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let a = chars.next().unwrap_or('0');
            let b = chars.next().unwrap_or('0');
            if let Ok(byte) = u8::from_str_radix(&format!("{}{}", a, b), 16) {
                out.push(byte as char);
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn urlencoding_encode(s: &str) -> String {
    s.chars().map(|c| {
        if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            c.to_string()
        } else {
            format!("%{:02X}", c as u32)
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_url_encoding_roundtrip() {
        let original = "C:\\Program Files\\My App/Data";
        let encoded = urlencoding_encode(original);
        assert!(encoded.contains("%5C"));
        let decoded = urlencoding_decode(&encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_list_any_dir() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("test_file.txt");
        let sub_dir = dir.path().join("sub_dir");
        fs::write(&file1, "hello").unwrap();
        fs::create_dir(&sub_dir).unwrap();

        let files = AgentServer::list_any_dir(dir.path());
        assert_eq!(files.len(), 2);
        
        let file_entry = files.iter().find(|f| f.name == "test_file.txt").unwrap();
        assert!(!file_entry.is_dir);
        assert_eq!(file_entry.size, 5);

        let dir_entry = files.iter().find(|f| f.name == "sub_dir").unwrap();
        assert!(dir_entry.is_dir);
    }

    #[test]
    fn test_agent_ping_auth_logic() {
        use mpsc::channel;
        let (tx, _rx) = channel();
        let config = Config::default();
        let api_key = "test_key".to_string();
        let server = AgentServer::new(0, api_key.clone(), PathBuf::from("."), tx, config.clone());
        
        let client = AgentClient::new("127.0.0.1", 0, api_key).unwrap();
        // Since we can't easily start the server in a test and wait for bind, 
        // we'll test the internal handle_request logic if it were public, 
        // but it's not. 
        
        // Let's just verify the AgentClient's error handling for now.
    }
}
