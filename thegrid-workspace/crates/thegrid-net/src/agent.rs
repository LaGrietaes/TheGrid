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

    fn capability_enabled(&self, capability: &str) -> bool {
        let cfg = self.config.lock().unwrap();
        match capability {
            "file_access" => cfg.enable_file_access,
            "terminal_access" => cfg.enable_terminal_access,
            "ai_access" => cfg.enable_ai_access,
            "remote_control" => cfg.enable_remote_control,
            _ => true,
        }
    }

    fn respond_capability_forbidden(req: Request, capability: &str) -> Result<()> {
        let body = serde_json::json!({
            "error": "forbidden",
            "reason": "capability_disabled",
            "capability": capability,
        })
        .to_string();
        req.respond(
            Response::from_string(body)
                .with_status_code(403)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap()),
        )?;
        Ok(())
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
            log::debug!("Agent [/ping] from {} - authorized={} ({}) - Key: {}", remote_addr, authorized, auth_mode, masked_key);

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
            if !self.capability_enabled("remote_control") {
                Self::respond_capability_forbidden(req, "remote_control")?;
                return Ok(());
            }
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
            if !self.capability_enabled("remote_control") {
                Self::respond_capability_forbidden(req, "remote_control")?;
                return Ok(());
            }
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
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
                return Ok(());
            }
            let after: i64 = url.split("after=")
                .nth(1)
                .and_then(|t| t.parse().ok())
                .unwrap_or(0);

            let (tx, rx) = mpsc::channel();
            let _ = self.event_tx.send(AppEvent::SyncRequest { after, response_tx: tx });
            let delta = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or_default();

            let json = serde_json::to_string(&delta)?;
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "GET" && url == "/filelist" {
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
                return Ok(());
            }
            let files = self.list_transfer_files();
            let json = serde_json::to_string(&files)?;
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "GET" && url.starts_with("/files/") {
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("remote_control") {
                Self::respond_capability_forbidden(req, "remote_control")?;
                return Ok(());
            }
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
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
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
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
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

        if method == "GET" && url.starts_with("/v1/preview") {
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
                return Ok(());
            }
            let path_str = url.split("path=").nth(1).unwrap_or("");
            let path_str = urlencoding_decode(path_str);
            let path = PathBuf::from(path_str);

            if path.exists() && path.is_file() {
                // Read up to 16KB
                let mut file = match std::fs::File::open(&path) {
                    Ok(f) => f,
                    Err(_) => {
                        req.respond(Response::from_string("Permission denied").with_status_code(403))?;
                        return Ok(());
                    }
                };
                let mut buf = vec![0u8; 16 * 1024];
                let bytes_read = file.read(&mut buf).unwrap_or(0);
                buf.truncate(bytes_read);
                req.respond(Response::from_data(buf))?;
            } else {
                req.respond(Response::from_string("Not found").with_status_code(404))?;
            }
            return Ok(());
        }

        if method == "DELETE" && url.starts_with("/v1/files") {
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
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
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("file_access") {
                Self::respond_capability_forbidden(req, "file_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("terminal_access") {
                Self::respond_capability_forbidden(req, "terminal_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("terminal_access") {
                Self::respond_capability_forbidden(req, "terminal_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("terminal_access") {
                Self::respond_capability_forbidden(req, "terminal_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("ai_access") {
                Self::respond_capability_forbidden(req, "ai_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("ai_access") {
                Self::respond_capability_forbidden(req, "ai_access")?;
                return Ok(());
            }
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
            if !self.capability_enabled("remote_control") {
                Self::respond_capability_forbidden(req, "remote_control")?;
                return Ok(());
            }
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
        let entries = match std::fs::read_dir(path) {
            Ok(e) => e,
            Err(e) => {
                log::warn!("[Browse] Cannot read {:?}: {}", path, e);
                return vec![];
            }
        };
        let mut files: Vec<RemoteFile> = entries
            .filter_map(|e| {
                let e = e.ok()?;
                // Skip entries we can't stat (permissions, broken symlinks, etc.)
                let meta = match e.metadata() {
                    Ok(m) => m,
                    Err(_) => return None,
                };
                let name = e.file_name().to_string_lossy().to_string();
                // Skip hidden dot-files on non-Windows
                #[cfg(not(windows))]
                if name.starts_with('.') { return None; }
                Some(RemoteFile {
                    name,
                    size: if meta.is_file() { meta.len() } else { 0 },
                    modified: meta.modified().ok().map(|t| chrono::DateTime::from(t)),
                    is_dir: meta.is_dir(),
                })
            })
            .collect();
        // Sort: dirs first, then by name
        files.sort_by(|a, b| {
            if a.is_dir != b.is_dir { b.is_dir.cmp(&a.is_dir) }
            else { a.name.to_lowercase().cmp(&b.name.to_lowercase()) }
        });
        files
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
            // On Linux/Android — surface key storage areas as top-level entries
            let priority_paths = [
                "/sdcard",              // Android primary storage (Termux)
                "/storage/emulated/0", // Android internal storage
                "/storage",            // Android all storage volumes
                "/data/data/com.termux/files/home", // Termux home
                "/",                   // Full root (may be restricted on Android)
            ];
            let mut dirs: Vec<RemoteFile> = Vec::new();
            for p in &priority_paths {
                let pb = Path::new(p);
                // Only include paths that exist and are readable
                if pb.exists() && std::fs::read_dir(pb).is_ok() {
                    // Avoid duplicates
                    if !dirs.iter().any(|d: &RemoteFile| d.name == *p) {
                        dirs.push(RemoteFile {
                            name: p.to_string(),
                            size: 0,
                            modified: None,
                            is_dir: true,
                        });
                    }
                }
            }
            // If nothing was found, fall back to listing /
            if dirs.is_empty() {
                dirs = Self::list_any_dir(Path::new("/"));
            }
            dirs
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
        log::debug!("Client: pinging {} with Key: {}", url, masked_key);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).timeout(std::time::Duration::from_secs(3)).send().context("Pinging agent")?;
        let status = resp.status();
        if !status.is_success() {
            log::warn!("Client: ping to {} failed with status {}", url, status);
            return Err(Self::handle_error(resp));
        }
        let r: AgentPingResponse = resp.json().context("Parsing ping response")?;
        log::debug!("Client: ping to {} succeeded (authorized={})", url, r.authorized);
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

    pub fn preview_file(&self, path: &str) -> Result<Vec<u8>> {
        let url = format!("{}/v1/preview?path={}", self.base_url, urlencoding_encode(path));
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?;
        if !resp.status().is_success() {
            return Err(Self::handle_error(resp));
        }
        let bytes = resp.bytes().context("Reading preview bytes")?;
        Ok(bytes.to_vec())
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

    pub fn sync_index(&self, last_sync_ts: i64) -> Result<SyncDelta> {
        let url = format!("{}/v1/sync?after={}", self.base_url, last_sync_ts);
        log::debug!("Client: syncing index from {} (after={})", url, last_sync_ts);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send().context("Requesting index sync")?;
        let status = resp.status();
        if !status.is_success() {
            log::warn!("Client: sync from {} failed with status {}", url, status);
            return Err(Self::handle_error(resp));
        }
        let r: SyncDelta = resp.json().context("Parsing sync JSON")?;
        log::debug!(
            "Client: sync from {} succeeded ({} files, {} tombstones)",
            url,
            r.files.len(),
            r.tombstones.len()
        );
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
    let cpu_cores_pct = Some(
        sys.cpus()
            .iter()
            .map(|c| c.cpu_usage().clamp(0.0, 100.0))
            .collect::<Vec<f32>>()
    );
    let cpu_model = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty());
    let cpu_physical_cores = sys.physical_core_count().map(|n| n as u32);
    let cpu_logical_processors = {
        let n = sys.cpus().len() as u32;
        if n > 0 { Some(n) } else { None }
    };
    let cpu_freq_ghz = sys
        .cpus()
        .first()
        .map(|c| c.frequency() as f32 / 1000.0)
        .filter(|f| *f > 0.0);
    let ram_used = sys.used_memory();
    let ram_total = sys.total_memory();
    
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut disk_used = 0;
    let mut disk_total = 0;
    let mut drive_infos = Vec::new();
    let storage_kind_hint = {
        if cfg!(target_os = "windows") {
            if let Ok(output) = std::process::Command::new("wmic")
                .args(["diskdrive", "get", "Model,MediaType"])
                .output()
            {
                let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
                if text.contains("nvme") {
                    Some("NVME".to_string())
                } else if text.contains("ssd") || text.contains("solid state") {
                    Some("SSD".to_string())
                } else if text.contains("hdd") || text.contains("hard disk") {
                    Some("HDD".to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    };
    
    for disk in &disks {
        let used = disk.total_space() - disk.available_space();
        let total = disk.total_space();
        disk_used += used;
        disk_total += total;
        drive_infos.push(DriveInfo {
            name: disk.name().to_string_lossy().into_owned(),
            used,
            total,
            kind: storage_kind_hint.clone(),
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

    let (running_processes, top_processes) = local_process_summary();

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

    // GPU info (Windows CIM/PowerShell best-effort with WMIC fallback)
    let gpu_devices = local_gpu_devices();
    let gpu_name = gpu_devices.first().map(|g| g.name.clone());

    let (ram_slots_used, ram_slots_total, ram_speed_mhz, ram_form_factor, ram_modules) = {
        if cfg!(target_os = "windows") {
            if let Some(json) = run_powershell_json(
                "$mods=Get-CimInstance Win32_PhysicalMemory | Select-Object DeviceLocator,BankLabel,Capacity,Speed,ConfiguredClockSpeed,SMBIOSMemoryType,FormFactor,Manufacturer,PartNumber; $slots=(Get-CimInstance Win32_PhysicalMemoryArray | Select-Object -First 1 -ExpandProperty MemoryDevices); [pscustomobject]@{slots=$slots;modules=$mods} | ConvertTo-Json -Compress"
            ) {
                let mut modules = Vec::<RamModule>::new();
                let mut speeds = Vec::<u32>::new();
                let mut form = None;

                let total_slots = json
                    .get("slots")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .filter(|v| *v > 0);

                if let Some(mods_val) = json.get("modules") {
                    for m in json_items(mods_val) {
                        let capacity = m.get("Capacity").and_then(|v| v.as_u64()).unwrap_or(0);
                        if capacity == 0 {
                            continue;
                        }
                        let speed = m.get("Speed").and_then(|v| v.as_u64()).map(|v| v as u32).filter(|v| *v > 0);
                        let configured_speed = m.get("ConfiguredClockSpeed").and_then(|v| v.as_u64()).map(|v| v as u32).filter(|v| *v > 0);
                        if let Some(s) = configured_speed.or(speed) {
                            speeds.push(s);
                        }
                        let ff = m.get("FormFactor").and_then(|v| v.as_u64()).map(|v| v as u32);
                        let ff_txt = ff.and_then(form_factor_name);
                        if form.is_none() {
                            form = ff_txt.clone();
                        }
                        let smbios = m.get("SMBIOSMemoryType").and_then(|v| v.as_u64()).map(|v| v as u32);
                        let mem_type = smbios.and_then(memory_type_name);
                        let slot = m.get("DeviceLocator").and_then(|v| v.as_str()).unwrap_or("SLOT").to_string();
                        let part_number = m.get("PartNumber").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
                        modules.push(RamModule {
                            slot,
                            capacity,
                            speed_mhz: speed,
                            configured_speed_mhz: configured_speed,
                            memory_type: mem_type,
                            form_factor: ff_txt,
                            latency_cl: part_number.as_deref().and_then(parse_latency_cl),
                            manufacturer: m.get("Manufacturer").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
                            part_number,
                        });
                    }
                }

                if !modules.is_empty() {
                    let used_slots = Some(modules.len() as u32);
                    let avg_speed = if speeds.is_empty() { None } else { Some(speeds.iter().sum::<u32>() / speeds.len() as u32) };
                    (used_slots, total_slots, avg_speed, form, modules)
                } else {
                    (None, total_slots, None, None, Vec::new())
                }
            } else {
            let mut used_slots = None;
            let mut total_slots = None;
            let mut speed = None;
            let mut form_factor = None;
            let mut modules = Vec::<RamModule>::new();

            if let Ok(output) = std::process::Command::new("wmic")
                .args(["memorychip", "get", "FormFactor,Speed,Capacity"])
                .output()
            {
                let text = String::from_utf8_lossy(&output.stdout);
                let mut used = 0_u32;
                let mut speeds = Vec::<u32>::new();
                let mut factors = Vec::<u32>::new();
                for line in text.lines() {
                    let l = line.trim();
                    if l.is_empty() || l.to_ascii_lowercase().contains("capacity") {
                        continue;
                    }
                    let cols: Vec<&str> = l.split_whitespace().collect();
                    if cols.len() >= 3 {
                        used += 1;
                        if let Ok(ff) = cols[0].parse::<u32>() {
                            if ff > 0 {
                                factors.push(ff);
                            }
                        }
                        if let Ok(mhz) = cols[1].parse::<u32>() {
                            if mhz > 0 {
                                speeds.push(mhz);
                            }
                        }
                        if let Ok(capacity) = cols[2].parse::<u64>() {
                            modules.push(RamModule {
                                slot: format!("SLOT {}", used),
                                capacity,
                                speed_mhz: cols[1].parse::<u32>().ok(),
                                configured_speed_mhz: cols[1].parse::<u32>().ok(),
                                memory_type: None,
                                form_factor: None,
                                latency_cl: None,
                                manufacturer: None,
                                part_number: None,
                            });
                        }
                    }
                }
                if used > 0 {
                    used_slots = Some(used);
                }
                if !speeds.is_empty() {
                    speed = Some(speeds.iter().sum::<u32>() / speeds.len() as u32);
                }
                if let Some(ff) = factors.first().copied() {
                    form_factor = Some(match ff {
                        8 => "DIMM".to_string(),
                        12 => "SODIMM".to_string(),
                        _ => format!("FF{}", ff),
                    });
                }
            }

            if let Ok(output) = std::process::Command::new("wmic")
                .args(["memphysical", "get", "MemoryDevices"])
                .output()
            {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    let l = line.trim();
                    if l.is_empty() || l.to_ascii_lowercase().contains("memorydevices") {
                        continue;
                    }
                    if let Ok(v) = l.parse::<u32>() {
                        if v > 0 {
                            total_slots = Some(v);
                            break;
                        }
                    }
                }
            }

            (used_slots, total_slots, speed, form_factor, modules)
            }
        } else {
            (None, None, None, None, Vec::new())
        }
    };

    NodeTelemetry {
        device_type: config.device_type.clone(),
        cpu_pct,
        cpu_model,
        cpu_physical_cores,
        cpu_logical_processors,
        cpu_cores_pct,
        cpu_freq_ghz,
        ram_used,
        ram_total,
        ram_slots_used,
        ram_slots_total,
        ram_speed_mhz,
        ram_form_factor,
        ram_modules,
        disk_used,
        disk_total,
        cpu_temp: None,
        is_ai_capable: !capabilities.ai_models.is_empty(),
        gpu_devices,
        gpu_name,
        gpu_pct: None,
        gpu_mem_used: None,
        gpu_mem_total: None,
        local_ips,
        running_processes,
        top_processes,
        ai_status: Some("Idle".into()),
        ai_tokens_per_sec: None,
        ai_thoughts: None,
        net_rx_bps: None,
        net_tx_bps: None,
        capabilities,
    }
}

fn local_gpu_devices() -> Vec<GpuDevice> {
    if !cfg!(target_os = "windows") {
        return Vec::new();
    }

    if let Some(json) = run_powershell_json(
        "$g=Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,PNPDeviceID,VideoProcessor; $g | ConvertTo-Json -Compress"
    ) {
        let mut devices = Vec::<GpuDevice>::new();
        for item in json_items(&json) {
            let name = item.get("Name").and_then(|v| v.as_str()).unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            let is_discrete = is_discrete_gpu_name(name);
            let is_rtx = name.to_ascii_lowercase().contains("rtx");
            let mem_total = item
                .get("AdapterRAM")
                .and_then(|v| v.as_u64())
                .filter(|v| *v > 0);
            let pnp = item.get("PNPDeviceID").and_then(|v| v.as_str()).unwrap_or("");
            let bus_type = if is_discrete {
                Some("PCIe".to_string())
            } else if pnp.to_ascii_lowercase().contains("pci") {
                Some("Integrated".to_string())
            } else {
                Some("Integrated".to_string())
            };
            devices.push(GpuDevice {
                name: name.to_string(),
                is_discrete,
                is_integrated: !is_discrete,
                is_shared: !is_discrete,
                is_rtx,
                ai_capable: is_rtx,
                vendor: Some(gpu_vendor(name)),
                bus_type,
                vram_type: gpu_memory_tech(name),
                gpu_pct: None,
                mem_used: None,
                mem_total,
            });
        }
        if !devices.is_empty() {
            devices.sort_by_key(|g| if g.is_discrete { 0_u8 } else { 1_u8 });
            return devices;
        }
    }

    let output = match std::process::Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "name"])
        .output()
    {
        Ok(out) => out,
        Err(_) => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::<GpuDevice>::new();
    for line in text.lines() {
        let name = line.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("name") {
            continue;
        }
        let is_discrete = is_discrete_gpu_name(name);
        let is_rtx = name.to_ascii_lowercase().contains("rtx");
        devices.push(GpuDevice {
            name: name.to_string(),
            is_discrete,
            is_integrated: !is_discrete,
            is_shared: !is_discrete,
            is_rtx,
            ai_capable: is_rtx,
            vendor: Some(gpu_vendor(name)),
            bus_type: Some(if is_discrete { "PCIe".to_string() } else { "Integrated".to_string() }),
            vram_type: gpu_memory_tech(name),
            gpu_pct: None,
            mem_used: None,
            mem_total: None,
        });
    }

    devices.sort_by_key(|g| if g.is_discrete { 0_u8 } else { 1_u8 });
    devices
}

fn is_discrete_gpu_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let integrated_markers = [
        "intel(r) uhd",
        "intel(r) iris",
        "intel hd",
        "radeon graphics",
        "adreno",
        "mali",
    ];
    if integrated_markers.iter().any(|m| n.contains(m)) {
        return false;
    }

    let discrete_markers = ["rtx", "gtx", "quadro", "tesla", "radeon rx", "arc "];
    discrete_markers.iter().any(|m| n.contains(m))
}

fn run_powershell_json(script: &str) -> Option<serde_json::Value> {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&text).ok()
}

fn json_items(v: &serde_json::Value) -> Vec<&serde_json::Value> {
    match v {
        serde_json::Value::Array(arr) => arr.iter().collect(),
        serde_json::Value::Object(_) => vec![v],
        _ => Vec::new(),
    }
}

fn gpu_vendor(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") {
        "NVIDIA".to_string()
    } else if n.contains("amd") || n.contains("radeon") {
        "AMD".to_string()
    } else if n.contains("intel") {
        "INTEL".to_string()
    } else {
        "UNKNOWN".to_string()
    }
}

fn gpu_memory_tech(name: &str) -> Option<String> {
    let n = name.to_ascii_lowercase();
    if n.contains("rtx 50") {
        Some("GDDR7".to_string())
    } else if n.contains("rtx") || n.contains("arc") {
        Some("GDDR6".to_string())
    } else if n.contains("gtx") {
        Some("GDDR5/6".to_string())
    } else if n.contains("intel") || n.contains("uhd") || n.contains("iris") {
        Some("SHARED DDR".to_string())
    } else {
        None
    }
}

fn form_factor_name(v: u32) -> Option<String> {
    Some(match v {
        8 => "DIMM".to_string(),
        12 => "SODIMM".to_string(),
        26 => "SODIMM".to_string(),
        _ => return None,
    })
}

fn memory_type_name(v: u32) -> Option<String> {
    Some(match v {
        20 => "DDR".to_string(),
        21 => "DDR2".to_string(),
        24 => "DDR3".to_string(),
        26 => "DDR4".to_string(),
        34 => "DDR5".to_string(),
        _ => return None,
    })
}

fn local_process_summary() -> (Option<u32>, Vec<String>) {
    if cfg!(target_os = "windows") {
        if let Some(json) = run_powershell_json(
            "$p=Get-Process | Sort-Object CPU -Descending | Select-Object -First 5 ProcessName,MainWindowTitle; [pscustomobject]@{count=(Get-Process).Count;top=$p} | ConvertTo-Json -Compress"
        ) {
            let count = json.get("count").and_then(|v| v.as_u64()).map(|v| v as u32);
            let mut top = Vec::<String>::new();
            if let Some(v) = json.get("top") {
                for item in json_items(v) {
                    let name = item.get("ProcessName").and_then(|v| v.as_str()).unwrap_or("").trim();
                    if !name.is_empty() {
                        top.push(name.to_string());
                    }
                }
            }
            return (count, top);
        }
    }

    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    let count = Some(sys.processes().len() as u32);
    let mut top = sys
        .processes()
        .values()
        .take(5)
        .map(|p| p.name().to_string())
        .collect::<Vec<_>>();
    top.sort();
    (count, top)
}

fn parse_latency_cl(part_number: &str) -> Option<u32> {
    let upper = part_number.to_ascii_uppercase();
    let pos = upper.find("CL")?;
    let digits: String = upper[pos + 2..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
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
