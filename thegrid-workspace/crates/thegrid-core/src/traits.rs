use std::path::Path;
use anyhow::Result;
use crate::utils::ScannedFile;

// ── FileScanner ───────────────────────────────────────────────────────────────

/// Abstracts directory scanning so tests can inject fixture data.
pub trait FileScanner: Send + Sync {
    fn scan_directory(&self, root: &Path) -> Result<Vec<ScannedFile>>;
}

/// Production scanner backed by std::fs::read_dir.
pub struct RealFileScanner;

impl FileScanner for RealFileScanner {
    fn scan_directory(&self, root: &Path) -> Result<Vec<ScannedFile>> {
        crate::utils::collect_files_in_directory(root)
    }
}

// ── ContentHasher ─────────────────────────────────────────────────────────────

/// Abstracts content hashing so tests can return pre-computed values instantly.
pub trait ContentHasher: Send + Sync {
    fn quick_hash(&self, path: &Path) -> Result<String>;
    fn full_hash(&self, path: &Path) -> Result<String>;
}

/// Production hasher backed by BLAKE3.
pub struct Blake3Hasher;

impl ContentHasher for Blake3Hasher {
    fn quick_hash(&self, path: &Path) -> Result<String> {
        crate::utils::quick_hash_file(path)
    }
    fn full_hash(&self, path: &Path) -> Result<String> {
        crate::utils::hash_file(path)
    }
}
