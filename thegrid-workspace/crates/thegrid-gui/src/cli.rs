// ═══════════════════════════════════════════════════════════════════════════════
// cli.rs — Command-line argument parser for TheGrid
//
// Used for Windows Explorer shell integration (right-click context menu).
// The exe is registered in HKCU under:
//   Directory\shell    → folder right-click (Scan, Ingest)
//   *\shell            → any file right-click (Open in TheGrid)
//   Directory\Background\shell → blank-area right-click (Scan here)
//
// Supported arguments:
//   thegrid.exe --scan   <path>   Scan/index a folder then show dashboard
//   thegrid.exe --ingest <path>   Open Media Ingest view filtered to this folder
//   thegrid.exe --open   <file>   Open Media Ingest with this file selected
//   thegrid.exe --minimize        Start minimised to system-tray / background
//
// All flags are optional; no flag → normal app boot.
// ═══════════════════════════════════════════════════════════════════════════════

use std::path::PathBuf;

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LaunchArgs {
    /// `--scan <path>` — index this directory, then stay on Dashboard
    pub scan_path:    Option<PathBuf>,
    /// `--ingest <path>` — open Media Ingest filtered to this directory
    pub ingest_path:  Option<PathBuf>,
    /// `--open <file>` — open Media Ingest with this specific file preselected
    pub open_file:    Option<PathBuf>,
    /// `--minimize` — start minimised (used from Explorer right-click background jobs)
    pub start_minimized: bool,
}

impl LaunchArgs {
    /// Parse `std::env::args()` into `LaunchArgs`.
    /// Unknown flags are silently ignored so future flags are backwards-compatible.
    pub fn parse() -> Self {
        let mut args = std::env::args().skip(1); // skip the binary name
        let mut out = LaunchArgs::default();

        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--scan" => {
                    if let Some(val) = args.next() {
                        out.scan_path = Some(PathBuf::from(val));
                    }
                }
                "--ingest" => {
                    if let Some(val) = args.next() {
                        out.ingest_path = Some(PathBuf::from(val));
                    }
                }
                "--open" => {
                    if let Some(val) = args.next() {
                        out.open_file = Some(PathBuf::from(val));
                    }
                }
                "--minimize" => {
                    out.start_minimized = true;
                }
                _ => {} // forward-compatible: ignore unknown flags
            }
        }

        out
    }

    /// Returns true when any shell-integration flag was supplied.
    pub fn has_shell_args(&self) -> bool {
        self.scan_path.is_some() || self.ingest_path.is_some() || self.open_file.is_some()
    }
}
