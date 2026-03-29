
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
