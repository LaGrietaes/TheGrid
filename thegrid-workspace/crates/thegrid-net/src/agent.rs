use anyhow::{Context, Result};
use reqwest::blocking::Client;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tiny_http::{Server, Response, Request};
use thegrid_core::{AppEvent, models::*, Config};
use ascii::AsciiStr;

const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

use std::sync::Arc;
use crate::tailscale::TailscaleClient;

pub struct AgentServer {
    port: u16,
    api_key: String,
    transfers_dir: PathBuf,
    event_tx: mpsc::Sender<AppEvent>,
    ts_client: Option<Arc<TailscaleClient>>,
    config: Config,
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
        }
    }

    pub fn with_tailscale(mut self, client: Arc<TailscaleClient>) -> Self {
        self.ts_client = Some(client);
        self
    }

    pub fn spawn(self) {
        let port = self.port;
        // Move self into the thread — this is why we don't need Arc<Self>
        std::thread::Builder::new()
            .name("thegrid-agent".into())
            .spawn(move || {
                if let Err(e) = self.run() {
                    log::error!("AgentServer fatal error: {}", e);
                }
            })
            .expect("Spawning agent thread");
        log::info!("THE GRID agent server started on port {}", port);
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
        log::debug!("Agent {} {}", method, url);

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

        // ── NEW: Remote Config Update ──
        if method == "POST" && url == "/v1/config" {
            let mut body = String::new();
            req.as_reader().read_to_string(&mut body)?;

            #[derive(serde::Deserialize)]
            struct ConfigUpdate {
                ai_model: Option<String>,
                ai_provider_url: Option<String>,
            }

            if let Ok(update) = serde_json::from_str::<ConfigUpdate>(&body) {
                let mut cfg = self.config.clone();
                if update.ai_model.is_some() { cfg.ai_model = update.ai_model; }
                if update.ai_provider_url.is_some() { cfg.ai_provider_url = update.ai_provider_url; }
                
                if let Err(e) = cfg.save() {
                    log::error!("Failed to save remote config update: {}", e);
                    req.respond(Response::from_string(format!(r#"{{"error":"{}"}}"#, e)).with_status_code(500))?;
                } else {
                    req.respond(Response::from_string(r#"{"ok":true}"#)
                        .with_header(tiny_http::Header::from_bytes(b"Content-Type", b"application/json").unwrap())
                    )?;
                }
            } else {
                req.respond(Response::from_string(r#"{"error":"invalid json"}"#).with_status_code(400))?;
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
            self.list_any_dir(Path::new("/"))
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
