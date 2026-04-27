// ═══════════════════════════════════════════════════════════════════════════════
// thegrid-core — The Kernel
//
// This crate has ZERO GUI and ZERO network code. It owns:
//   1. Data models shared across all crates
//   2. User config with disk persistence
//   3. The SQLite database layer (Tier A index — Phase 3 completes this)
//   4. The AppEvent enum — the message bus between background threads and GUI
//
// Every other crate depends on this one. Keep it lean.
// ═══════════════════════════════════════════════════════════════════════════════

pub mod config;
pub mod db;
pub mod events;
pub mod models;
pub mod watcher;
pub mod utils;
pub mod traits;
pub mod indexing;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

// Re-export the most-used types so callers can `use thegrid_core::*`
pub use config::Config;
pub use db::{Database, should_skip_dir, should_skip_path, unix_now};
pub use events::AppEvent;
pub use models::*;
pub use watcher::FileWatcher;
pub use utils::{collect_files_in_directory, fingerprint_file, hash_file, match_rules, quick_hash_file, scan_and_index_directory};
pub use traits::{Blake3Hasher, ContentHasher, FileScanner, RealFileScanner};
pub use indexing::classify_directory;
