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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            device_name: String::new(),
            rdp_username: String::new(),
            agent_port: Self::default_agent_port(),
            watch_paths: Vec::new(),
            transfers_dir: None,
        }
    }
}

impl Config {
    fn default_agent_port() -> u16 { 47731 }

    /// Returns the path to the config file, creating the directory if needed.
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir()
            .context("Could not determine config directory")?;
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
        let cfg: Self = serde_json::from_str(&raw)
            .context("Parsing config JSON")?;
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
