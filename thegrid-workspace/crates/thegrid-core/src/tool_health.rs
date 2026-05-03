// ═══════════════════════════════════════════════════════════════════════════
// tool_health.rs — Runtime Capability Tier + External Tool Status
//
// Defines which tiers of media processing are available based on what
// external tools (ffmpeg, gyroflow, ollama) are installed and reachable.
//
// Tier table (from MEDIA_CARE_DEPENDENCY_MATRIX_V1.md):
//   T0  image-only ops      — always available (pure-Rust `image` crate)
//   T1  ffmpeg present      — video/audio transforms, thumbnails, preview
//   T2  ffmpeg + vad/denoise — advanced audio cleanup
//   T3  transcription / AI  — Ollama / cloud AI recommendations
//   T4  gyro stabilisation  — Gyroflow external adapter
// ═══════════════════════════════════════════════════════════════════════════

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Highest media-processing tier currently satisfied by installed tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub enum CapabilityTier {
    #[default]
    T0Image       = 0,
    T1Ffmpeg      = 1,
    T2VadDenoise  = 2,
    T3Transcription = 3,
    T4Gyroflow    = 4,
}

impl CapabilityTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::T0Image         => "Tier 0 — Image only",
            Self::T1Ffmpeg        => "Tier 1 — Video/Audio (ffmpeg)",
            Self::T2VadDenoise    => "Tier 2 — Advanced Audio",
            Self::T3Transcription => "Tier 3 — AI Transcription",
            Self::T4Gyroflow      => "Tier 4 — Gyro Stabilization",
        }
    }

    /// RGB accent colour for the tier badge.
    pub fn color_rgb(self) -> [u8; 3] {
        match self {
            Self::T0Image         => [120, 120, 120],
            Self::T1Ffmpeg        => [60,  180,  80],
            Self::T2VadDenoise    => [60,  130, 230],
            Self::T3Transcription => [180, 100, 230],
            Self::T4Gyroflow      => [230, 160,  30],
        }
    }
}

/// Status of a single external tool after probing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolStatus {
    /// Found and responded to a version/ping query.
    Ok { version: String, path: PathBuf },
    /// Not found on PATH or via env-var override.
    Missing { hint: String },
    /// Found but failed to execute (bad binary, permissions, etc.).
    Error { message: String },
}

impl ToolStatus {
    pub fn is_ok(&self)      -> bool { matches!(self, Self::Ok { .. }) }
    pub fn is_missing(&self) -> bool { matches!(self, Self::Missing { .. }) }

    pub fn short_label(&self) -> &str {
        match self {
            Self::Ok { version, .. } => version.as_str(),
            Self::Missing { .. }     => "MISSING",
            Self::Error { .. }       => "ERROR",
        }
    }

    pub fn install_hint(&self) -> Option<&str> {
        match self {
            Self::Missing { hint } => Some(hint.as_str()),
            _                      => None,
        }
    }
}

/// Full report emitted by the runtime once at startup (and optionally on-demand).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolHealthReport {
    pub probed_at: std::time::SystemTime,
    pub ffmpeg:    ToolStatus,
    pub ffprobe:   ToolStatus,
    pub gyroflow:  ToolStatus,
    pub ollama:    ToolStatus,
    pub fabric:    ToolStatus,
    /// Highest tier whose prerequisites are all satisfied.
    pub tier:      CapabilityTier,
}

impl ToolHealthReport {
    /// All (tool_name, hint) pairs where tools are missing.
    pub fn missing_hints(&self) -> Vec<(&'static str, &str)> {
        [
            ("ffmpeg",   &self.ffmpeg),
            ("ffprobe",  &self.ffprobe),
            ("ollama",   &self.ollama),
            ("gyroflow", &self.gyroflow),
            ("fabric",   &self.fabric),
        ]
        .iter()
        .filter_map(|(name, status)| status.install_hint().map(|h| (*name, h)))
        .collect()
    }

    /// True when at least ffmpeg + ffprobe are present.
    pub fn has_ffmpeg(&self) -> bool {
        self.ffmpeg.is_ok() && self.ffprobe.is_ok()
    }
}
