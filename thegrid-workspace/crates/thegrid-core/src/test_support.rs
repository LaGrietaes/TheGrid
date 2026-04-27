/// Test doubles for FileScanner and ContentHasher.
/// Available under `#[cfg(test)]` or the `test-support` feature flag.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use anyhow::Result;

use crate::traits::{ContentHasher, FileScanner};
use crate::utils::ScannedFile;
use crate::models::FileFingerprint;

// ── FixtureScanner ────────────────────────────────────────────────────────────

/// Scanner that returns a pre-set list of ScannedFile entries.
/// Call .push() to seed the fixture before use.
pub struct FixtureScanner {
    files: Vec<ScannedFile>,
}

impl FixtureScanner {
    pub fn new(files: Vec<ScannedFile>) -> Self {
        Self { files }
    }

    /// Build from a slice of (path, size) pairs — quick_hash is derived from the path string.
    pub fn from_paths(entries: &[(&str, u64)]) -> Self {
        let files = entries
            .iter()
            .map(|(p, size)| ScannedFile {
                path: PathBuf::from(p),
                fingerprint: FileFingerprint {
                    size: *size,
                    modified: Some(1_700_000_000),
                    quick_hash: Some(format!("qhash_{}", p.replace(['/', '\\', ':'], "_"))),
                },
            })
            .collect();
        Self { files }
    }
}

impl FileScanner for FixtureScanner {
    fn scan_directory(&self, _root: &Path) -> Result<Vec<ScannedFile>> {
        Ok(self.files.clone())
    }
}

// ── RecordingHasher ───────────────────────────────────────────────────────────

/// Hasher that returns pre-computed hashes from a map and records every call.
/// Paths not in the map get a deterministic fallback hash based on the path string.
pub struct RecordingHasher {
    map: HashMap<PathBuf, String>,
    calls: Mutex<Vec<PathBuf>>,
}

impl RecordingHasher {
    pub fn new(map: HashMap<PathBuf, String>) -> Self {
        Self {
            map,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Build from a slice of (path, hash) string pairs.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        let map = pairs
            .iter()
            .map(|(p, h)| (PathBuf::from(p), h.to_string()))
            .collect();
        Self::new(map)
    }

    /// Returns every path that was requested so far.
    pub fn calls(&self) -> Vec<PathBuf> {
        self.calls.lock().unwrap().clone()
    }

    fn record(&self, path: &Path) {
        self.calls.lock().unwrap().push(path.to_path_buf());
    }

    fn lookup(&self, path: &Path) -> String {
        self.map
            .get(path)
            .cloned()
            .unwrap_or_else(|| format!("fixture_hash_{}", path.to_string_lossy().replace(['/', '\\', ':'], "_")))
    }
}

impl ContentHasher for RecordingHasher {
    fn quick_hash(&self, path: &Path) -> Result<String> {
        self.record(path);
        Ok(self.lookup(path))
    }
    fn full_hash(&self, path: &Path) -> Result<String> {
        self.record(path);
        Ok(self.lookup(path))
    }
}

// ── NullEmbedder (thegrid-ai lives in a separate crate; we mirror the trait here for
//    cross-crate tests that only need deterministic output) ─────────────────────

/// Returns a fixed zero-vector for any input. Records call count.
pub struct NullEmbedder {
    dims: usize,
    call_count: Mutex<usize>,
}

impl NullEmbedder {
    pub fn new(dims: usize) -> Self {
        Self { dims, call_count: Mutex::new(0) }
    }

    pub fn call_count(&self) -> usize {
        *self.call_count.lock().unwrap()
    }

    pub fn embed(&self, _text: &str) -> Result<Vec<f32>> {
        *self.call_count.lock().unwrap() += 1;
        Ok(vec![0.0_f32; self.dims])
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Create a temporary in-memory Database ready for tests.
pub fn in_memory_db() -> Result<crate::db::Database> {
    crate::db::Database::open(":memory:")
}

/// Seed a DB with n files on a given device, all needing hashes.
pub fn seed_files(db: &crate::db::Database, device_id: &str, paths: &[(&str, u64)]) -> Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(paths.len());
    for (p, size) in paths {
        let id = db.index_file_with_source(
            device_id,
            "TestDevice",
            &PathBuf::from(p),
            *size,
            Some(1_700_000_000),
            Some(&format!("qhash_{}", p.replace(['/', '\\', ':'], "_"))),
            None,
            crate::models::DetectionSource::FullScan,
            crate::db::unix_now(),
        )?;
        ids.push(id);
    }
    Ok(ids)
}

/// Set a known full hash on a previously seeded file by path.
pub fn set_hash(db: &crate::db::Database, id: i64, hash: &str) -> Result<()> {
    db.update_file_hash(id, hash)
}
