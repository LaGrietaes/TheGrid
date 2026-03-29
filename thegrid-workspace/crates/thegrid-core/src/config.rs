use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// All user-configurable settings for THE GRID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Tailscale API key (tskey-api-...). Required to list devices.
    #[serde(default)]
    pub api_key: String,

    /// Human-readable label for THIS machine (e.g., "WORKSTATION-MAIN")
    #[serde(default)]
    pub device_name: String,

    /// Device type classification (Desktop, Laptop, Tablet, Smartphone, Server, NAS, Board)
    #[serde(default = "Config::default_device_type")]
    pub device_type: String,

    /// Default Windows username for RDP sessions
    #[serde(default)]
    pub rdp_username: String,

    /// Port the local THE GRID agent HTTP server listens on
    #[serde(default = "Config::default_agent_port")]
    pub agent_port: u16,

    /// Phase 2: User-defined directories to watch and index
    #[serde(default)]
    pub watch_paths: Vec<PathBuf>,

    /// Directory where received files are saved
    #[serde(default)]
    pub transfers_dir: Option<PathBuf>,

    /// Specific local AI model for this device (e.g. "llama3:8b")
    #[serde(default)]
    pub ai_model: Option<String>,

    /// API URL for the AI provider (e.g. local Ollama server)
    #[serde(default)]
    pub ai_provider_url: Option<String>,

    /// Enable RDP access on this node
    #[serde(default = "Config::default_true")]
    pub enable_rdp: bool,

    /// Enable remote file access over the mesh
    #[serde(default = "Config::default_true")]
    pub enable_file_access: bool,

    /// Enable remote terminal endpoints over the mesh
    #[serde(default = "Config::default_true")]
    pub enable_terminal_access: bool,

    /// Enable remote AI inference endpoints over the mesh
    #[serde(default = "Config::default_true")]
    pub enable_ai_access: bool,

    /// Enable remote control operations (config mutation, adb, privileged controls)
    #[serde(default = "Config::default_true")]
    pub enable_remote_control: bool,

    /// Phase 3: User-defined Custom Smart Rules for filtering files
    #[serde(default)]
    pub smart_rules: Vec<crate::models::SmartRule>,

    /// Phase 3: User-defined Projects
    #[serde(default)]
    pub projects: Vec<crate::models::Project>,

    /// Phase 3: User-defined Categories
    #[serde(default)]
    pub categories: Vec<crate::models::Category>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: "tskey-api-kMA88YTvnk11CNTRL-GwpDrsDdtrRYKvV7TAeNsRHTsjrH5dG8".to_string(),
            device_name: "0N3-DEV".to_string(),
            device_type: Self::default_device_type(),
            rdp_username: "0N3-DEV".to_string(),
            agent_port: Self::default_agent_port(),
            watch_paths: Vec::new(),
            transfers_dir: None,
            ai_model: None,
            ai_provider_url: None,
            enable_rdp: true,
            enable_file_access: true,
            enable_terminal_access: true,
            enable_ai_access: true,
            enable_remote_control: true,
            smart_rules: Vec::new(),
            projects: vec![
                crate::models::Project { id: "p1".into(), name: "THE GRID".into(), description: "Core System".into(), tags: vec!["#core".into(), "#system".into()] },
                crate::models::Project { id: "p2".into(), name: "RECON".into(), description: "Active Scanning".into(), tags: vec!["#net".into()] },
            ],
            categories: vec![
                crate::models::Category { id: "c1".into(), name: "DOCUMENTS".into(), icon: "📄".into() },
                crate::models::Category { id: "c2".into(), name: "MEDIA".into(), icon: "🎞".into() },
            ],
        }
    }
}

impl Config {
    fn default_agent_port() -> u16 {
        5000
    }
    fn default_true() -> bool {
        true
    }
    fn default_device_type() -> String {
        "Desktop".to_string()
    }

    /// Returns the path to the config file, creating the directory if needed.
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().context("Could not determine config directory")?;
        let dir = base.join("thegrid");
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join("config.json"))
    }

    /// Load config from disk. Returns `Config::default()` if none exists yet.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            log::info!("No config found at {:?} — using defaults", path);
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Reading config from {:?}", path))?;
        let mut cfg: Self = serde_json::from_str(&raw).context("Parsing config JSON")?;

        // Normalize and trim API key
        cfg.api_key = cfg.api_key.trim().to_string();

        // Fallback to hardcoded key if loaded one is empty
        if cfg.api_key.is_empty() {
            cfg.api_key =
                "tskey-api-kMA88YTvnk11CNTRL-GwpDrsDdtrRYKvV7TAeNsRHTsjrH5dG8".to_string();
        }

        // Migrate names to 0N3-DEV as requested
        if cfg.device_name == "DEV-ON3" || cfg.device_name.is_empty() {
            cfg.device_name = "0N3-DEV".to_string();
        }
        if cfg.rdp_username == "0N3-DEV" || cfg.rdp_username.is_empty() {
             cfg.rdp_username = "0N3-DEV".to_string();
        }

        log::info!("Config loaded from {:?}", path);
        Ok(cfg)
    }

    /// Persist config to disk. Overwrites any existing file.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        log::info!("Config saved to {:?}", path);
        Ok(())
    }

    /// True only if we have a non-empty API key to work with.
    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// Returns the transfers directory, defaulting to %APPDATA%\thegrid\transfers
    pub fn effective_transfers_dir(&self) -> PathBuf {
        if let Some(d) = &self.transfers_dir {
            return d.clone();
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("thegrid")
            .join("transfers")
    }
}
