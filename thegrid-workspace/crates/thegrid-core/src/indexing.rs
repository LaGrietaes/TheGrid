/// Classifies directories before scanning to avoid wasting resources on volatile
/// or recoverable project trees (node_modules, target/, git-backed repos, etc.).
use std::path::Path;
use crate::models::{IndexingOverride, IndexingTier, OverrideAction};

// ── Tier 0: hard-excluded directory names ─────────────────────────────────────

/// Directories that are unconditionally excluded — always non-indexable.
const TIER0_NAMES: &[&str] = &[
    "node_modules",
    ".pnpm-store",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".next",
    ".nuxt",
    "dist",
    "build",
    ".gradle",
    ".m2",
    ".cargo",
    ".rustup",
    "vendor",
];

/// Sub-paths within a `.git` directory that are all binary object data.
const GIT_OBJECT_DIRS: &[&str] = &["objects", "logs", "refs"];

// ── Classification ────────────────────────────────────────────────────────────

/// Classify a directory before scanning it.
/// Returns the appropriate tier, logging the reason.
///
/// Override rules are checked first (exact match, then glob); the first match wins.
pub fn classify_directory(path: &Path, overrides: &[IndexingOverride]) -> (IndexingTier, String) {
    let path_str = path.to_string_lossy();

    // 1. User overrides (exact match first)
    for ov in overrides {
        let matches = if ov.path_pattern.contains('*') || ov.path_pattern.contains('?') {
            glob_matches(&ov.path_pattern, &path_str)
        } else {
            path_str.starts_with(&ov.path_pattern) || path_str.eq_ignore_ascii_case(&ov.path_pattern)
        };

        if matches {
            let (tier, reason) = match ov.action {
                OverrideAction::ForceInclude     => (IndexingTier::FullIndex, format!("user override: force-include ({})", ov.path_pattern)),
                OverrideAction::ForceExclude     => (IndexingTier::Tier0Exclude, format!("user override: force-exclude ({})", ov.path_pattern)),
                OverrideAction::MetadataOnly     => (IndexingTier::Tier1Deprioritized, format!("user override: metadata-only ({})", ov.path_pattern)),
                OverrideAction::DeprioritizeTier1 => (IndexingTier::Tier1Deprioritized, format!("user override: deprioritized ({})", ov.path_pattern)),
            };
            return (tier, reason);
        }
    }

    // 2. Tier 0 hard exclusions by directory name
    if let Some(name) = path.file_name() {
        let name_str = name.to_string_lossy();
        if TIER0_NAMES.iter().any(|t0| name_str.eq_ignore_ascii_case(t0)) {
            return (
                IndexingTier::Tier0Exclude,
                format!("tier-0 excluded directory name: {}", name_str),
            );
        }

        // .git/objects, .git/logs, .git/refs — object store is binary noise
        if let Some(parent) = path.parent() {
            if parent.file_name().map_or(false, |p| p.eq_ignore_ascii_case(".git")) {
                if GIT_OBJECT_DIRS.iter().any(|d| name_str.eq_ignore_ascii_case(d)) {
                    return (
                        IndexingTier::Tier0Exclude,
                        format!("git internal directory: .git/{}", name_str),
                    );
                }
            }
        }

        // target/ inside a Rust workspace (has sibling Cargo.toml)
        if name_str.eq_ignore_ascii_case("target") {
            if let Some(parent) = path.parent() {
                if parent.join("Cargo.toml").exists() || parent.join("Cargo.lock").exists() {
                    return (
                        IndexingTier::Tier0Exclude,
                        "Rust build output (target/ next to Cargo.toml)".to_string(),
                    );
                }
            }
        }
    }

    // 3. GitHub-backed detection: .git/config with a github.com remote
    let git_config_path = path.join(".git").join("config");
    if git_config_path.exists() {
        if let Some(remote) = read_git_remote_url(&git_config_path) {
            if remote.contains("github.com") {
                return (
                    IndexingTier::GitHubBacked,
                    format!("git working copy with GitHub remote: {}", remote),
                );
            }
            // Non-GitHub git repo → still deprioritize (large object count, recoverable)
            return (
                IndexingTier::Tier1Deprioritized,
                format!("git working copy (non-GitHub remote: {})", remote),
            );
        }
        // .git exists but could not read remote
        return (
            IndexingTier::Tier1Deprioritized,
            "git working copy (remote unknown)".to_string(),
        );
    }

    // 4. Tier 1 by project marker (dev trees: likely high churn)
    let project_markers = ["Cargo.toml", "package.json", "pyproject.toml", "pom.xml", "build.gradle", "CMakeLists.txt"];
    for marker in &project_markers {
        if path.join(marker).exists() {
            return (
                IndexingTier::Tier1Deprioritized,
                format!("development project root (contains {})", marker),
            );
        }
    }

    (IndexingTier::FullIndex, "no exclusion rule matched".to_string())
}

/// Parse the first `[remote "origin"]` url from a `.git/config` file.
/// Does not shell out — reads the file as plain text.
fn read_git_remote_url(git_config: &Path) -> Option<String> {
    let content = std::fs::read_to_string(git_config).ok()?;
    let mut in_remote_section = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[remote") {
            in_remote_section = true;
        } else if trimmed.starts_with('[') {
            in_remote_section = false;
        } else if in_remote_section {
            if let Some(url_part) = trimmed.strip_prefix("url =") {
                return Some(url_part.trim().to_string());
            }
            if let Some(url_part) = trimmed.strip_prefix("url=") {
                return Some(url_part.trim().to_string());
            }
        }
    }
    None
}

/// Minimal glob matcher supporting `*` (any chars in segment) and `**` (any depth).
fn glob_matches(pattern: &str, path: &str) -> bool {
    let pat = pattern.replace('\\', "/");
    let p = path.replace('\\', "/");
    glob_match_parts(pat.split('/').collect::<Vec<_>>().as_slice(), p.split('/').collect::<Vec<_>>().as_slice())
}

fn glob_match_parts(pat: &[&str], path: &[&str]) -> bool {
    match (pat.first(), path.first()) {
        (None, None)    => true,
        (None, _)       => false,
        (Some(&"**"), _) => {
            // ** matches zero or more path segments
            (0..=path.len()).any(|skip| glob_match_parts(&pat[1..], &path[skip..]))
        }
        (_, None) => false,
        (Some(p), Some(s)) => {
            segment_matches(p, s) && glob_match_parts(&pat[1..], &path[1..])
        }
    }
}

fn segment_matches(pattern: &str, segment: &str) -> bool {
    if pattern == "*" { return true; }
    if !pattern.contains('*') { return pattern.eq_ignore_ascii_case(segment); }

    let parts: Vec<&str> = pattern.splitn(2, '*').collect();
    if parts.len() != 2 { return false; }
    let (before, after) = (parts[0], parts[1]);
    let s_lower = segment.to_lowercase();
    let b_lower = before.to_lowercase();
    let a_lower = after.to_lowercase();
    s_lower.starts_with(&b_lower) && s_lower[b_lower.len()..].ends_with(&a_lower)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn tier0_node_modules() {
        let tmp = TempDir::new().unwrap();
        let nm = tmp.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        let (tier, reason) = classify_directory(&nm, &[]);
        assert_eq!(tier, IndexingTier::Tier0Exclude, "node_modules must be Tier0: {}", reason);
    }

    #[test]
    fn tier0_rust_target() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("Cargo.toml"), "[package]").unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        let (tier, _) = classify_directory(&target, &[]);
        assert_eq!(tier, IndexingTier::Tier0Exclude, "target/ next to Cargo.toml must be Tier0");
    }

    #[test]
    fn tier1_cargo_toml_project_root() {
        let tmp = TempDir::new().unwrap();
        // A separate crate root (not rust target)
        let proj = tmp.path().join("myapp");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("Cargo.toml"), "[package]").unwrap();
        let (tier, _) = classify_directory(&proj, &[]);
        assert_eq!(tier, IndexingTier::Tier1Deprioritized, "project root with Cargo.toml must be Tier1");
    }

    #[test]
    fn github_backed_detected_from_git_config() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("config"), "[remote \"origin\"]\n\turl = https://github.com/user/repo.git\n").unwrap();
        let (tier, reason) = classify_directory(tmp.path(), &[]);
        assert_eq!(tier, IndexingTier::GitHubBacked, "should detect GitHub remote: {}", reason);
    }

    #[test]
    fn non_github_git_is_tier1() {
        let tmp = TempDir::new().unwrap();
        let git_dir = tmp.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("config"), "[remote \"origin\"]\n\turl = https://gitlab.com/user/repo.git\n").unwrap();
        let (tier, _) = classify_directory(tmp.path(), &[]);
        assert_eq!(tier, IndexingTier::Tier1Deprioritized, "non-GitHub git should be Tier1");
    }

    #[test]
    fn plain_directory_is_full_index() {
        let tmp = TempDir::new().unwrap();
        let (tier, _) = classify_directory(tmp.path(), &[]);
        assert_eq!(tier, IndexingTier::FullIndex, "plain directory should be FullIndex");
    }

    #[test]
    fn user_override_force_exclude_wins() {
        let tmp = TempDir::new().unwrap();
        let overrides = vec![IndexingOverride {
            path_pattern: tmp.path().to_string_lossy().to_string(),
            action: OverrideAction::ForceExclude,
        }];
        let (tier, _) = classify_directory(tmp.path(), &overrides);
        assert_eq!(tier, IndexingTier::Tier0Exclude, "force-exclude override must win");
    }

    #[test]
    fn user_override_force_include_beats_tier0_name() {
        let tmp = TempDir::new().unwrap();
        let nm = tmp.path().join("node_modules");
        fs::create_dir_all(&nm).unwrap();
        let overrides = vec![IndexingOverride {
            path_pattern: nm.to_string_lossy().to_string(),
            action: OverrideAction::ForceInclude,
        }];
        let (tier, _) = classify_directory(&nm, &overrides);
        assert_eq!(tier, IndexingTier::FullIndex, "force-include must override tier-0 name");
    }
}
