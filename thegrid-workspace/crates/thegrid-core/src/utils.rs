
use std::path::Path;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use anyhow::Result;

use crate::models::FileFingerprint;

/// Compute the BLAKE3 hash of a file at the given path.
/// This reads the file in chunks to avoid loading large files into memory.
pub fn hash_file(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = ::blake3::Hasher::new();
    let mut buffer = [0u8; 65536]; // 64KB chunks

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute a fast identity hash by sampling the head and tail of the file.
pub fn quick_hash_file(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let mut hasher = ::blake3::Hasher::new();
    let mut buffer = [0u8; 16 * 1024];

    hasher.update(&len.to_le_bytes());

    let head_len = reader.read(&mut buffer)?;
    hasher.update(&buffer[..head_len]);

    if len > buffer.len() as u64 {
        let tail_start = len.saturating_sub(buffer.len() as u64);
        reader.seek(SeekFrom::Start(tail_start))?;
        let tail_len = reader.read(&mut buffer)?;
        hasher.update(&buffer[..tail_len]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

pub fn fingerprint_file(path: &Path) -> Result<FileFingerprint> {
    let metadata = path.metadata()?;
    let modified = metadata.modified().ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64);

    Ok(FileFingerprint {
        size: metadata.len(),
        modified,
        quick_hash: quick_hash_file(path).ok(),
    })
}

/// Match a file path against a set of user rules.
/// Returns a list of RuleMatch results for any active rules that match.
pub fn match_rules(path: &Path, rules: &[crate::models::UserRule], file_id: i64) -> Vec<crate::models::RuleMatch> {
    use glob::Pattern;
    let mut matches = Vec::new();
    let path_str = path.to_string_lossy();

    for rule in rules {
        if !rule.is_active { continue; }
        
        if let Ok(pattern) = Pattern::new(&rule.pattern) {
            if pattern.matches(&path_str) {
                matches.push(crate::models::RuleMatch {
                    rule_id: rule.id,
                    file_id,
                    tag: rule.tag.clone(),
                    project: rule.project.clone(),
                });
            }
        }
    }
    matches
}

// ── Directory Scanning & Indexing ─────────────────────────────────────────
use std::path::PathBuf;
use std::sync::mpsc;
use crate::events::AppEvent;
use crate::db::Database;

/// A file entry discovered during directory scanning.
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub fingerprint: FileFingerprint,
}

/// Recursively scan a directory and collect file information.
/// Skips common build/cache/vcs directories and hidden files.
fn scan_directory_recursive(
    root: &Path,
    current: &Path,
    collected: &mut Vec<ScannedFile>,
    scanned_count: &mut u64,
) -> Result<()> {
    // Read directory entries
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()), // Skip inaccessible directories
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();

        // Skip common build/cache directories and hidden files
        if crate::db::should_skip_dir(&name_str) {
            continue;
        }

        if path.is_dir() {
            // Recurse into subdirectories
            let _ = scan_directory_recursive(root, &path, collected, scanned_count);
        } else if path.is_file() {
            // Fingerprint the file
            if let Ok(fingerprint) = fingerprint_file(&path) {
                collected.push(ScannedFile { path, fingerprint });
                *scanned_count += 1;
            }
        }
    }

    Ok(())
}

/// Scan a directory and batch-insert files into the database.
/// Emits IndexProgress events for UI feedback and IndexComplete when done.
pub fn scan_and_index_directory(
    root: &Path,
    device_id: &str,
    device_name: &str,
    db: &Database,
    event_tx: &mpsc::Sender<AppEvent>,
) -> Result<()> {
    let start_time = std::time::Instant::now();
    log::info!("[SCAN] Starting index of {:?}", root);

    let mut collected = Vec::new();
    let mut scanned_count = 0u64;

    // Phase 1: Recursively collect all files
    scan_directory_recursive(root, root, &mut collected, &mut scanned_count)?;

    let total_files = collected.len() as u64;
    log::info!("[SCAN] Found {} files in {:?}", total_files, root);

    // Phase 2: Batch insert into database with progress reporting
    let batch_size = 500usize;
    let mut inserted = 0u64;

    for (idx, batch) in collected.chunks(batch_size).enumerate() {
        let mut batch_current = String::new();
        let mut batch_ext = None;
        
        // Insert this batch
        for scanned_file in batch {
            if let Err(e) = db.index_file_with_source(
                device_id,
                device_name,
                &scanned_file.path,
                scanned_file.fingerprint.size,
                scanned_file.fingerprint.modified,
                scanned_file.fingerprint.quick_hash.as_deref(),
                None, // full hash will be computed separately in background
                crate::models::DetectionSource::FullScan,
                crate::db::unix_now(),
            ) {
                log::warn!("Failed to index {:?}: {}", scanned_file.path, e);
                continue;
            }
            inserted += 1;
            batch_current = scanned_file.path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            batch_ext = scanned_file.path.extension()
                .map(|e| e.to_string_lossy().to_string());
        }

        // Emit progress every batch
        let percent = ((inserted as f32 / total_files as f32) * 100.0).min(99.9);
        
        let _ = event_tx.send(AppEvent::IndexProgress {
            scanned: scanned_count,
            total: total_files,
            current: batch_current,
            ext: batch_ext,
            estimated_total: false,
        });

        if idx % 2 == 0 {
            log::debug!("[SCAN] Progress: {}/{} ({}%)", inserted, total_files, percent as u32);
        }
    }

    let duration_ms = start_time.elapsed().as_millis() as u64;
    log::info!("[SCAN] Complete: {} files indexed in {}ms", inserted, duration_ms);

    // Emit final completion event
    let _ = event_tx.send(AppEvent::IndexComplete {
        device_id: device_id.to_string(),
        files_added: inserted,
        duration_ms,
    });

    Ok(())
}
