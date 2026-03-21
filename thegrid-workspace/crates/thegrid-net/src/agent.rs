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

use thegrid_core::{AppEvent, models::*, Config};
use ascii::AsciiStr;

const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");


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
    config: Config,
    terminal_sessions: Mutex<HashMap<String, TerminalSession>>,
}

impl AgentServer {
    pub fn new(
        port: u16,
        api_key: String,
        transfers_dir: PathBuf,
        event_tx: mpsc::Sender<AppEvent>,
        config: Config,
    ) -> Self {
        Self { 
            port, 
            api_key, 
            transfers_dir, 
            event_tx,
            ts_client: None,
            config,
            terminal_sessions: Mutex::new(HashMap::new()),
        }
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

        for request in server.incoming_requests() {
            if let Err(e) = self.handle_request(request) {
                log::warn!("Agent request error: {}", e);
            }
        }
        Ok(())
    }

    fn handle_request(&self, mut req: Request) -> Result<()> {
        let method = req.method().to_string();
        let url    = req.url().to_string();
        log::info!("Agent {} {}", method, url);

        if url != "/ping" {
            let mut authorized = false;

            // 1. Check for X-Grid-Key header
            let key = req.headers().iter()
                .find(|h| h.field.as_str().eq_ignore_ascii_case(AsciiStr::from_ascii("X-Grid-Key").unwrap()))
                .map(|h| h.value.as_str());
            
            if key == Some(&self.api_key) {
                authorized = true;
            }

            // 2. If not authorized by key, check Tailscale trust
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
                log::warn!("Agent: unauthorized access attempt from {:?}", req.remote_addr());
                req.respond(Response::from_string(r#"{"error":"unauthorized"}"#)
                    .with_status_code(401)
                    .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                )?;
                return Ok(());
            }
        }

        if method == "GET" && url == "/ping" {
            let h = hostname::get().unwrap_or_else(|_| std::ffi::OsString::from("UNKNOWN")).to_string_lossy().to_string();
            let body = serde_json::json!({
                "ok": true,
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

        if method == "GET" && url == "/telemetry" {
            let telemetry = collect_telemetry(&self.config);
            let json = serde_json::to_string(&telemetry).unwrap_or_default();
            req.respond(Response::from_string(json)
                .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
            )?;
            return Ok(());
        }

        if method == "POST" && url == "/adb/enable" {
            #[cfg(target_os = "linux")]
            {
                log::info!("Agent: attempting to enable ADB over TCP/IP 5555 (Termux)");
                match std::process::Command::new("adb")
                    .arg("tcpip").arg("5555")
                    .output() 
                {
                    Ok(output) => {
                        let success = output.status.success();
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        let msg = if success { 
                             "ADB 5555 enabled".to_string() 
                        } else { 
                            format!("Failed to enable ADB: {}{}", stdout, stderr) 
                        };
                        
                        if success { log::info!("{}", msg); } else { log::error!("{}", msg); }

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
            if !self.config.enable_file_access {
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
            if !self.config.enable_file_access {
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

        // ── NEW: Remote Config Update ──

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
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).timeout(std::time::Duration::from_secs(3)).send().context("Pinging agent")?;
        resp.json().context("Parsing ping response")
    }

    pub fn list_files(&self) -> Result<Vec<RemoteFile>> {
        #[derive(serde::Deserialize)]
        struct Resp { files: Vec<RemoteFile> }
        let url = format!("{}/filelist", self.base_url);
        let resp: Resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?.json()?;
        Ok(resp.files)
    }

    pub fn download_file(&self, filename: &str, dest_dir: &Path) -> Result<PathBuf> {
        let url = format!("{}/files/{}", self.base_url, urlencoding_encode(filename));
        let bytes = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?.bytes().context("Reading file bytes")?;
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
            return Err(anyhow::anyhow!("Upload returned {}", resp.status()));
        }
        Ok(())
    }

    pub fn send_clipboard(&self, content: &str, sender: &str) -> Result<()> {
        let url = format!("{}/clipboard", self.base_url);
        let body = serde_json::json!({ "content": content, "sender": sender });
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).json(&body).send()?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Clipboard relay returned {}", resp.status()));
        }
        Ok(())
    }

    pub fn create_terminal_session(&self) -> Result<String> {
        let url = format!("{}/v1/terminal/session", self.base_url);
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).send()?;
        #[derive(serde::Deserialize)]
        struct Resp { session_id: String }
        let r: Resp = resp.json()?;
        Ok(r.session_id)
    }

    pub fn send_terminal_input(&self, session_id: &str, data: &[u8]) -> Result<()> {
        let url = format!("{}/v1/terminal/input?id={}", self.base_url, session_id);
        let resp = self.http.post(&url).header("X-Grid-Key", &self.api_key).body(data.to_vec()).send()?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Terminal input failed: {}", resp.status()));
        }
        Ok(())
    }

    pub fn get_terminal_output(&self, session_id: &str) -> Result<Vec<u8>> {
        let url = format!("{}/v1/terminal/output?id={}", self.base_url, session_id);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?;
        let bytes = resp.bytes()?.to_vec();
        Ok(bytes)
    }

    pub fn get_telemetry(&self) -> Result<NodeTelemetry> {
        let url = format!("{}/telemetry", self.base_url);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).timeout(std::time::Duration::from_secs(5)).send().context("Fetching telemetry")?;
        resp.json().context("Parsing telemetry JSON")
    }

    pub fn sync_index(&self, last_sync_ts: i64) -> Result<Vec<FileSearchResult>> {
        let url = format!("{}/v1/sync?after={}", self.base_url, last_sync_ts);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send().context("Requesting index sync")?;
        if resp.status().is_success() {
            resp.json().context("Parsing sync JSON")
        } else {
            Err(anyhow::anyhow!("Sync failed: {}", resp.status()))
        }
    }

    pub fn browse_directory(&self, path: &Path) -> Result<Vec<RemoteFile>> {
        let path_str = urlencoding_encode(&path.to_string_lossy());
        let url = format!("{}/v1/browse?path={}", self.base_url, path_str);
        let resp = self.http.get(&url).header("X-Grid-Key", &self.api_key).send().context("Browsing remote directory")?;
        if resp.status().is_success() {
            resp.json().context("Parsing browse JSON")
        } else {
            Err(anyhow::anyhow!("Browse failed: {}", resp.status()))
        }
    }

    pub fn download_remote_file(&self, path: &Path, dest: &Path) -> Result<PathBuf> {
        let filename = path.file_name().ok_or_else(|| anyhow::anyhow!("Invalid path"))?.to_string_lossy();
        let path_str = urlencoding_encode(&path.to_string_lossy());
        let url = format!("{}/v1/read?path={}", self.base_url, path_str);
        let bytes = self.http.get(&url).header("X-Grid-Key", &self.api_key).send()?.bytes().context("Reading remote file bytes")?;
        
        let dest_file = dest.join(&*filename);
        std::fs::write(&dest_file, &bytes)?;
        Ok(dest_file)
    }

    pub fn remote_embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/v1/ai/embed", self.base_url);
        let body = serde_json::json!({ "text": text });
        let resp = self.http.post(&url)
            .header("X-Grid-Key", &self.api_key)
            .json(&body)
            .send()
            .context("Remote embedding request")?;
        
        if resp.status().is_success() {
            resp.json().context("Parsing remote embed response")
        } else {
            Err(anyhow::anyhow!("Remote embed failed: {}", resp.status()))
        }
    }

    pub fn remote_search(&self, query: &str, k: usize) -> Result<Vec<(i64, f32)>> {
        let url = format!("{}/v1/ai/search", self.base_url);
        let body = serde_json::json!({ "query": query, "k": k });
        let resp = self.http.post(&url)
            .header("X-Grid-Key", &self.api_key)
            .json(&body)
            .send()
            .context("Remote search request")?;
        
        if resp.status().is_success() {
            resp.json().context("Parsing remote search response")
        } else {
            Err(anyhow::anyhow!("Remote search failed: {}", resp.status()))
        }
    }

    pub fn update_config(&self, model: Option<String>, url: Option<String>) -> Result<()> {
        let endpoint = format!("{}/v1/config", self.base_url);
        let body = serde_json::json!({
            "ai_model": model,
            "ai_provider_url": url,
        });
        let resp = self.http.post(&endpoint)
            .header("X-Grid-Key", &self.api_key)
            .json(&body)
            .send()
            .context("Sending config update")?;
        
        if resp.status().is_success() {
            Ok(())
        } else {
            let err_body = resp.text().unwrap_or_else(|_| "Unknown error".to_string());
            Err(anyhow::anyhow!("Config update failed: {}", err_body))
        }
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
    let mut drive_names = Vec::new();
    
    for disk in &disks {
        disk_used += disk.total_space() - disk.available_space();
        disk_total += disk.total_space();
        drive_names.push(disk.name().to_string_lossy().into_owned());
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
        drives: drive_names,
        has_rdp: config.enable_rdp,
        has_file_access: config.enable_file_access,
    };

    NodeTelemetry {
        device_type: config.device_type.clone(),
        cpu_pct,
        ram_used,
        ram_total,
        disk_used,
        disk_total,
        cpu_temp: None,
        is_ai_capable: !capabilities.ai_models.is_empty(),
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
}
