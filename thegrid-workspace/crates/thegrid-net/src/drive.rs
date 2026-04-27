/// Google Drive metadata indexing client for THE GRID.
///
/// Flow:
///   1. Call `DriveClient::authorize()` — opens browser, starts loopback server,
///      waits for the OAuth2 redirect, exchanges code for tokens, persists them.
///   2. Call `index_all_files()` to page through Drive and index metadata.
///   3. Token refresh is automatic (access token expires in 1h).
///
/// Only metadata is fetched — no file content is downloaded.
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use thegrid_core::{
    AppEvent,
    models::{DriveAbout, DriveFileMetadata},
};

// ── OAuth2 constants ──────────────────────────────────────────────────────────

const OAUTH_AUTH_URL:  &str = "https://accounts.google.com/o/oauth2/v2/auth";
const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DRIVE_FILES_URL: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_ABOUT_URL: &str = "https://www.googleapis.com/drive/v3/about";

const REDIRECT_URI:    &str = "http://localhost:9876";
const LOOPBACK_PORT:   u16  = 9876;

const SCOPES: &str = "https://www.googleapis.com/auth/drive.metadata.readonly \
                       https://www.googleapis.com/auth/drive.readonly";

const PAGE_SIZE: u32 = 1000;

const FILE_FIELDS: &str =
    "nextPageToken,files(id,name,size,modifiedTime,md5Checksum,mimeType,parents)";

// ── Token persistence ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredTokens {
    access_token:  String,
    refresh_token: Option<String>,
    /// Unix timestamp (seconds) when the access token expires
    expires_at:    i64,
}

impl StoredTokens {
    fn is_expired(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        now >= self.expires_at - 60
    }
}

fn token_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("thegrid")
        .join("drive_tokens.json")
}

fn load_stored_tokens() -> Option<StoredTokens> {
    let path = token_path();
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn save_stored_tokens(tokens: &StoredTokens) -> Result<()> {
    let path = token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(tokens)?;
    std::fs::write(path, json)?;
    Ok(())
}

// ── OAuth2 response types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token:  String,
    refresh_token: Option<String>,
    expires_in:    Option<i64>,
    error:         Option<String>,
}

// ── Drive API response types ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct FilesListResponse {
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
    files: Vec<DriveApiFile>,
}

#[derive(Debug, Deserialize)]
struct DriveApiFile {
    id:   String,
    name: String,
    #[serde(rename = "mimeType")]
    mime_type: String,
    size: Option<String>,
    #[serde(rename = "modifiedTime")]
    modified_time: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(rename = "md5Checksum")]
    md5_checksum: Option<String>,
    parents: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct AboutResponse {
    user:        Option<AboutUser>,
    #[serde(rename = "storageQuota")]
    storage_quota: Option<AboutQuota>,
}

#[derive(Debug, Deserialize)]
struct AboutUser {
    #[serde(rename = "emailAddress")]
    email_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AboutQuota {
    usage:  Option<String>,
    limit:  Option<String>,
}

// ── DriveClient ───────────────────────────────────────────────────────────────

pub struct DriveClient {
    client_id:     String,
    client_secret: String,
    http:          reqwest::blocking::Client,
    tokens:        Option<StoredTokens>,
}

impl DriveClient {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id:     client_id.into(),
            client_secret: client_secret.into(),
            http:          reqwest::blocking::Client::new(),
            tokens:        load_stored_tokens(),
        }
    }

    pub fn is_authorized(&self) -> bool {
        self.tokens.is_some()
    }

    // ── Authorization ─────────────────────────────────────────────────────────

    /// Launch the OAuth2 flow:
    ///   1. Open the user's browser to the consent URL.
    ///   2. Start a loopback HTTP server on port 9876.
    ///   3. Wait up to 5 minutes for the redirect.
    ///   4. Exchange the authorization code for tokens.
    ///   5. Persist the tokens to disk.
    pub fn authorize(&mut self) -> Result<()> {
        let state = uuid::Uuid::new_v4().to_string();
        let auth_url = format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&access_type=offline&prompt=consent",
            OAUTH_AUTH_URL,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(REDIRECT_URI),
            urlencoding::encode(SCOPES),
            state,
        );

        // Open browser
        if let Err(e) = open_browser(&auth_url) {
            log::warn!("Could not open browser automatically: {}", e);
            log::info!("Open this URL manually:\n{}", auth_url);
        }

        // Start loopback server and wait for redirect
        let code = wait_for_oauth_code(LOOPBACK_PORT, &state, Duration::from_secs(300))
            .context("OAuth2 loopback listener timed out or failed")?;

        // Exchange code for tokens
        let tokens = self.exchange_code(&code)?;
        save_stored_tokens(&tokens)?;
        self.tokens = Some(tokens);
        Ok(())
    }

    fn exchange_code(&self, code: &str) -> Result<StoredTokens> {
        let resp: TokenResponse = self.http
            .post(OAUTH_TOKEN_URL)
            .form(&[
                ("client_id",     self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("redirect_uri",  REDIRECT_URI),
                ("grant_type",    "authorization_code"),
                ("code",          code),
            ])
            .send()?
            .json()?;

        if let Some(err) = resp.error {
            return Err(anyhow!("Token exchange failed: {}", err));
        }

        let expires_at = chrono::Utc::now().timestamp() + resp.expires_in.unwrap_or(3600);
        Ok(StoredTokens {
            access_token:  resp.access_token,
            refresh_token: resp.refresh_token,
            expires_at,
        })
    }

    fn refresh_access_token(&mut self) -> Result<()> {
        let refresh_token = self.tokens
            .as_ref()
            .and_then(|t| t.refresh_token.clone())
            .ok_or_else(|| anyhow!("No refresh token stored — re-authorize"))?;

        let resp: TokenResponse = self.http
            .post(OAUTH_TOKEN_URL)
            .form(&[
                ("client_id",     self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type",    "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
            ])
            .send()?
            .json()?;

        if let Some(err) = resp.error {
            return Err(anyhow!("Token refresh failed: {}", err));
        }

        let expires_at = chrono::Utc::now().timestamp() + resp.expires_in.unwrap_or(3600);
        let tokens = StoredTokens {
            access_token:  resp.access_token,
            refresh_token: resp.refresh_token.or_else(|| {
                self.tokens.as_ref().and_then(|t| t.refresh_token.clone())
            }),
            expires_at,
        };
        save_stored_tokens(&tokens)?;
        self.tokens = Some(tokens);
        Ok(())
    }

    fn access_token(&mut self) -> Result<String> {
        if let Some(t) = &self.tokens {
            if t.is_expired() {
                self.refresh_access_token()?;
            }
            return Ok(self.tokens.as_ref().unwrap().access_token.clone());
        }
        Err(anyhow!("Not authorized — call authorize() first"))
    }

    // ── Drive API ─────────────────────────────────────────────────────────────

    pub fn get_about(&mut self) -> Result<DriveAbout> {
        let token = self.access_token()?;
        let resp: AboutResponse = self.http
            .get(DRIVE_ABOUT_URL)
            .query(&[("fields", "user,storageQuota")])
            .bearer_auth(&token)
            .send()?
            .json()?;

        Ok(DriveAbout {
            email:         resp.user.and_then(|u| u.email_address).unwrap_or_default(),
            storage_used:  resp.storage_quota.as_ref().and_then(|q| q.usage.as_ref()).and_then(|s| s.parse().ok()).unwrap_or(0),
            storage_limit: resp.storage_quota.as_ref().and_then(|q| q.limit.as_ref()).and_then(|s| s.parse().ok()),
        })
    }

    /// Page through all Drive files and emit `DriveIndexProgress` events.
    /// Returns all file metadata collected.
    pub fn index_all_files(
        &mut self,
        event_tx: &mpsc::Sender<AppEvent>,
    ) -> Result<Vec<DriveFileMetadata>> {
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;
        let mut page_num = 0u64;

        loop {
            let token = self.access_token()?;

            let mut params = vec![
                ("pageSize",        PAGE_SIZE.to_string()),
                ("fields",          FILE_FIELDS.to_string()),
                ("q",               "trashed = false".to_string()),
                ("includeItemsFromAllDrives", "true".to_string()),
                ("supportsAllDrives",         "true".to_string()),
            ];
            if let Some(ref pt) = page_token {
                params.push(("pageToken", pt.clone()));
            }

            let resp = self.http
                .get(DRIVE_FILES_URL)
                .query(&params)
                .bearer_auth(&token)
                .send()
                .context("Drive files list request")?;

            if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
                self.refresh_access_token()?;
                continue;
            }

            let list: FilesListResponse = resp.json().context("Drive files list JSON decode")?;

            for f in &list.files {
                // Skip Google Docs/Sheets/Slides — no binary data to hash
                if f.mime_type.starts_with("application/vnd.google-apps.") {
                    continue;
                }
                let size: u64 = f.size.as_deref().and_then(|s| s.parse().ok()).unwrap_or(0);
                all_files.push(DriveFileMetadata {
                    id:             f.id.clone(),
                    name:           f.name.clone(),
                    size,
                    modified:       f.modified_time,
                    md5_checksum:   f.md5_checksum.clone(),
                    mime_type:      f.mime_type.clone(),
                    parents:        f.parents.clone().unwrap_or_default(),
                    is_shared_drive: false,
                });
            }

            page_num += 1;
            let indexed = all_files.len() as u64;
            let _ = event_tx.send(AppEvent::DriveIndexProgress {
                indexed,
                total: None,
            });

            log::debug!("[Drive] Page {}: {} files so far", page_num, indexed);

            page_token = list.next_page_token;
            if page_token.is_none() {
                break;
            }
        }

        let total = all_files.len() as u64;
        let _ = event_tx.send(AppEvent::DriveIndexComplete { indexed: total });
        log::info!("[Drive] Indexed {} files total", total);
        Ok(all_files)
    }
}

// ── OAuth2 loopback server ────────────────────────────────────────────────────

/// Wait for the browser to redirect to `http://localhost:{port}?code=...&state=...`.
/// Validates the returned state to prevent CSRF.
fn wait_for_oauth_code(port: u16, expected_state: &str, timeout: Duration) -> Result<String> {
    let server = tiny_http::Server::http(format!("0.0.0.0:{}", port))
        .map_err(|e| anyhow!("Could not bind loopback server on port {}: {}", port, e))?;

    let deadline = Instant::now() + timeout;

    loop {
        if Instant::now() > deadline {
            return Err(anyhow!("OAuth2 loopback timed out after {:?}", timeout));
        }

        let request = match server.recv_timeout(Duration::from_secs(5)) {
            Ok(Some(req)) => req,
            Ok(None) => continue,
            Err(e) => return Err(anyhow!("Loopback server error: {}", e)),
        };

        let url = request.url().to_string();

        // Parse query string
        let query = url.splitn(2, '?').nth(1).unwrap_or("");
        let mut code  = None;
        let mut state = None;

        for pair in query.split('&') {
            let mut kv = pair.splitn(2, '=');
            let k = kv.next().unwrap_or("");
            let v = kv.next().unwrap_or("");
            match k {
                "code"  => code  = Some(urlencoding::decode(v).unwrap_or_default().into_owned()),
                "state" => state = Some(urlencoding::decode(v).unwrap_or_default().into_owned()),
                _ => {}
            }
        }

        // Send browser response
        let html = if code.is_some() {
            "<html><body><h2>Authorization complete — you can close this tab.</h2></body></html>"
        } else {
            "<html><body><h2>Authorization failed — no code received.</h2></body></html>"
        };
        let response = tiny_http::Response::from_string(html)
            .with_header("Content-Type: text/html".parse::<tiny_http::Header>().unwrap());
        let _ = request.respond(response);

        match (code, state) {
            (Some(c), Some(s)) if s == expected_state => return Ok(c),
            (Some(_), Some(s)) => {
                return Err(anyhow!("OAuth2 state mismatch (got {}, expected {})", s, expected_state));
            }
            (None, _) => {
                return Err(anyhow!("OAuth2 redirect did not include a code"));
            }
            _ => continue,
        }
    }
}

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}
