use anyhow::{bail, Result};
use std::process::Command;

/// Resolution preset for RDP sessions.
#[derive(Debug, Clone, PartialEq)]
pub enum RdpResolution {
    /// Use the current monitor size (mstsc default)
    FullScreen,
    /// Custom width x height
    Custom(u32, u32),
}

impl RdpResolution {
    pub fn from_str(s: &str) -> Self {
        match s {
            "1920x1080" => Self::Custom(1920, 1080),
            "2560x1440" => Self::Custom(2560, 1440),
            "1280x800"  => Self::Custom(1280, 800),
            _           => Self::FullScreen,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::FullScreen       => "FULLSCREEN",
            Self::Custom(1920, _)  => "1920×1080",
            Self::Custom(2560, _)  => "2560×1440",
            Self::Custom(1280, _)  => "1280×800",
            Self::Custom(_, _)     => "CUSTOM",
        }
    }
}

pub struct RdpLauncher;

impl RdpLauncher {
    /// Launch mstsc.exe pointing at `ip`, optionally with credentials and
    /// resolution.
    pub fn launch(ip: &str, username: Option<&str>, resolution: &RdpResolution) -> Result<()> {
        #[cfg(not(target_os = "windows"))]
        {
            bail!(
                "RDP launch requires Windows (mstsc.exe). \
                 On Linux/macOS consider xfreerdp or Remmina — Phase 3 TODO."
            );
        }

        #[cfg(target_os = "windows")]
        {
            let mut cmd = Command::new("mstsc.exe");
            cmd.arg(format!("/v:{}", ip));

            if let Some(user) = username {
                if !user.is_empty() {
                    cmd.arg(format!("/u:{}", user));
                }
            }

            if let RdpResolution::Custom(w, h) = resolution {
                cmd.arg(format!("/w:{}", w));
                cmd.arg(format!("/h:{}", h));
            }

            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x00000008); // DETACHED_PROCESS

            cmd.spawn()
                .map_err(|e| anyhow::anyhow!("Failed to launch mstsc.exe: {}", e))?;

            log::info!("Launched mstsc.exe for {}", ip);
            Ok(())
        }
    }
}
