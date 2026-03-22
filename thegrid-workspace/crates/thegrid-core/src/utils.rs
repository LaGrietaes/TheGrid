
use std::path::Path;
use std::fs::File;
use std::io::{Read, BufReader};
use anyhow::Result;

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
