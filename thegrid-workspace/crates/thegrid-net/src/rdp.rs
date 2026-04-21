use anyhow::Result;
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
            let _ = (ip, username, resolution);
            anyhow::bail!(
                "RDP launch requires Windows (mstsc.exe). \
                 On Linux/macOS consider xfreerdp or Remmina — Phase 3 TODO."
            );
        }

        #[cfg(target_os = "windows")]
        {
            use std::io::Write;
            
            // On Windows, mstsc.exe does NOT support a `/u:` parameter for usernames.
            // To pass a username, we must write out a temporary .rdp file.
            let temp_dir = std::env::temp_dir();
            let rdp_path = temp_dir.join(format!("thegrid_{}.rdp", ip.replace(".", "_").replace(":", "_")));
            
            let mut file = std::fs::File::create(&rdp_path)
                .map_err(|e| anyhow::anyhow!("Failed to create temporary .rdp file: {}", e))?;
            
            // Build the .rdp content
            writeln!(file, "full address:s:{}", ip)?;
            if let Some(user) = username {
                if !user.trim().is_empty() {
                    writeln!(file, "username:s:{}", user)?;
                }
            }
            
            match resolution {
                RdpResolution::FullScreen => {
                    writeln!(file, "screen mode id:i:2")?; // Fullscreen
                }
                RdpResolution::Custom(w, h) => {
                    writeln!(file, "screen mode id:i:1")?; // Deskop window
                    writeln!(file, "desktopwidth:i:{}", w)?;
                    writeln!(file, "desktopheight:i:{}", h)?;
                }
            }
            
            // Optimization for high-latency/Tailscale connections
            writeln!(file, "compression:i:1")?;
            writeln!(file, "keyboardhook:i:2")?;
            writeln!(file, "audiomode:i:0")?;
            writeln!(file, "redirectclipboard:i:1")?;
            writeln!(file, "displayconnectionbar:i:1")?;
            
            // Force prompt for credentials to ensure a clean login if NLA fails
            // writeln!(file, "prompt for credentials:i:1")?; 

            let mut cmd = Command::new("mstsc.exe");
            cmd.arg(rdp_path.to_string_lossy().to_string());
            
            // Run the process directly. mstsc is a GUI app, so it won't open a console.
            cmd.spawn()
                .map_err(|e| anyhow::anyhow!("Failed to launch mstsc.exe with .rdp file: {}", e))?;

            log::info!("Launched mstsc.exe for {} with temp .rdp at {:?}", ip, rdp_path);
            Ok(())
        }
    }
}
