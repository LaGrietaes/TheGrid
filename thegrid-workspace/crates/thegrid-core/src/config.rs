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

    /// Use tablet offload as helper when tablet appears idle/low-usage.
    #[serde(default = "Config::default_true")]
    pub ai_tablet_assist: bool,

    /// Max CPU usage percentage on tablet to consider it available for assist.
    #[serde(default = "Config::default_tablet_cpu_max_pct")]
    pub ai_tablet_assist_cpu_max_pct: f32,

    /// Max GPU usage percentage on tablet to consider it available for assist.
    #[serde(default = "Config::default_tablet_gpu_max_pct")]
    pub ai_tablet_assist_gpu_max_pct: f32,

    /// Number of concurrent embedding requests per worker cycle.
    #[serde(default = "Config::default_embedding_parallel_requests")]
    pub embedding_parallel_requests: usize,

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

    /// Enable staging duplicates/canonicals into a local Drive buffer workspace.
    #[serde(default = "Config::default_true")]
    pub drive_buffer_enabled: bool,

    /// Soft quota used for UI/planning (subscription-backed target capacity).
    #[serde(default = "Config::default_drive_buffer_quota_tb")]
    pub drive_buffer_quota_tb: u32,

    /// Remote path used by upload tools like rclone, e.g. "gdrive:THEGRID-BUFFER".
    #[serde(default)]
    pub drive_buffer_remote: Option<String>,

    /// Local staging folder for preparing categorized files + manifests before upload.
    #[serde(default)]
    pub drive_buffer_root: Option<PathBuf>,

    /// User-defined indexing overrides (path patterns → actions).
    #[serde(default)]
    pub indexing_overrides: Vec<crate::models::IndexingOverride>,

    /// Allow delegating compute tasks to other devices on the Tailnet.
    #[serde(default = "Config::default_true")]
    pub allow_compute_borrowing: bool,

    /// Max number of compute tasks this device will handle simultaneously.
    #[serde(default = "Config::default_max_parallel_compute")]
    pub max_parallel_compute_tasks: u8,
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
            ai_tablet_assist: true,
            ai_tablet_assist_cpu_max_pct: Self::default_tablet_cpu_max_pct(),
            ai_tablet_assist_gpu_max_pct: Self::default_tablet_gpu_max_pct(),
            embedding_parallel_requests: Self::default_embedding_parallel_requests(),
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
            drive_buffer_enabled: true,
            drive_buffer_quota_tb: Self::default_drive_buffer_quota_tb(),
            drive_buffer_remote: Some("gdrive:THEGRID-BUFFER".to_string()),
            drive_buffer_root: None,
            indexing_overrides: Vec::new(),
            allow_compute_borrowing: true,
            max_parallel_compute_tasks: Self::default_max_parallel_compute(),
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
    fn default_tablet_cpu_max_pct() -> f32 {
        55.0
    }
    fn default_tablet_gpu_max_pct() -> f32 {
        60.0
    }
    fn default_embedding_parallel_requests() -> usize {
        3
    }
    fn default_drive_buffer_quota_tb() -> u32 {
        15
    }
    fn default_device_type() -> String {
        "Desktop".to_string()
    }
    fn default_max_parallel_compute() -> u8 {
        2
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

    /// Returns the local staging directory used for Drive buffer exports.
    pub fn effective_drive_buffer_dir(&self) -> PathBuf {
        if let Some(d) = &self.drive_buffer_root {
            return d.clone();
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("thegrid")
            .join("drive-buffer")
    }
}
