use anyhow::{Context, Result};
use reqwest::blocking::Client;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use tiny_http::{Server, Response, Request};
use thegrid_core::{AppEvent, models::*};
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
}

impl AgentServer {
    pub fn new(
        port: u16,
        api_key: String,
        transfers_dir: PathBuf,
        event_tx: mpsc::Sender<AppEvent>,
    ) -> Self {
        Self { 
            port, 
            api_key, 
            transfers_dir, 
            event_tx,
            ts_client: None,
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
            let telemetry = collect_telemetry();
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

        req.respond(Response::from_string("Not found").with_status_code(404))?;
        Ok(())
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
}

fn collect_telemetry() -> NodeTelemetry {
    NodeTelemetry::default()
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
