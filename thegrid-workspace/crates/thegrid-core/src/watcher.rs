use anyhow::Result;
use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::events::AppEvent;
use crate::models::{FileChange, FileChangeKind};
use crate::db::should_skip_path;
use crate::utils::fingerprint_file;

/// Watches filesystem paths and emits `AppEvent::FileSystemChanged`
pub struct FileWatcher {
    watcher: RecommendedWatcher,
    tx: mpsc::Sender<AppEvent>,
}

impl FileWatcher {
    pub fn new(event_tx: mpsc::Sender<AppEvent>) -> Result<Self> {
        let tx_clone = event_tx.clone();

        let watcher = notify::recommended_watcher(move |result: notify::Result<Event>| {
            match result {
                Ok(event) => {
                    let changes = normalize_event(event);
                    if changes.is_empty() {
                        return;
                    }

                    let summary = summarize_changes(&changes);
                    log::debug!("FileWatcher: {}", summary);
                    let _ = tx_clone.send(AppEvent::FileSystemChanged { changes, summary });
                }
                Err(err) => {
                    let msg = err.to_string();
                    log::error!("FileWatcher error: {}", msg);
                    let _ = tx_clone.send(AppEvent::FileWatcherError(msg));
                }
            }
        }).map_err(|e| anyhow::anyhow!("Creating file watcher: {}", e))?;

        Ok(Self { watcher, tx: event_tx })
    }

    pub fn watch(&mut self, path: PathBuf) -> Result<()> {
        self.watcher
            .watch(&path, RecursiveMode::Recursive)
            .map_err(|e| anyhow::anyhow!("Watching {:?}: {}", path, e))?;

        log::info!("FileWatcher: now watching {:?}", path);
        let _ = self.tx.send(AppEvent::Status(
            format!("Watching: {}", path.display())
        ));
        Ok(())
    }

    pub fn unwatch(&mut self, path: &PathBuf) -> Result<()> {
        self.watcher
            .unwatch(path)
            .map_err(|e| anyhow::anyhow!("Unwatching {:?}: {}", path, e))?;
        log::info!("FileWatcher: stopped watching {:?}", path);
        Ok(())
    }
}

fn normalize_event(event: Event) -> Vec<FileChange> {
    match event.kind {
        EventKind::Create(CreateKind::Any | CreateKind::File | CreateKind::Folder)
        | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Metadata(_)) => {
            event.paths.into_iter().filter_map(|path| {
                if should_drop_watcher_path(&path) {
                    return None;
                }
                Some(FileChange {
                    kind: if matches!(event.kind, EventKind::Create(_)) {
                        FileChangeKind::Created
                    } else {
                        FileChangeKind::Modified
                    },
                    fingerprint: build_fingerprint(&path),
                    old_path: None,
                    new_path: None,
                    path,
                })
            }).collect()
        }
        EventKind::Remove(RemoveKind::Any | RemoveKind::File | RemoveKind::Folder) => {
            event.paths.into_iter().filter_map(|path| {
                if should_drop_watcher_path(&path) {
                    return None;
                }
                Some(FileChange {
                    kind: FileChangeKind::Deleted,
                    path,
                    old_path: None,
                    new_path: None,
                    fingerprint: None,
                })
            }).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() >= 2 => {
            let old_path = event.paths[0].clone();
            let new_path = event.paths[1].clone();
            if should_drop_watcher_path(&old_path) || should_drop_watcher_path(&new_path) {
                return Vec::new();
            }
            vec![FileChange {
                kind: FileChangeKind::Renamed,
                path: new_path.clone(),
                old_path: Some(old_path),
                new_path: Some(new_path.clone()),
                fingerprint: build_fingerprint(&new_path),
            }]
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            event.paths.into_iter().filter_map(|path| {
                if should_drop_watcher_path(&path) {
                    return None;
                }
                Some(FileChange {
                    kind: FileChangeKind::Deleted,
                    path,
                    old_path: None,
                    new_path: None,
                    fingerprint: None,
                })
            }).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
            event.paths.into_iter().filter_map(|path| {
                if should_drop_watcher_path(&path) {
                    return None;
                }
                Some(FileChange {
                    kind: FileChangeKind::Created,
                    fingerprint: build_fingerprint(&path),
                    old_path: None,
                    new_path: None,
                    path,
                })
            }).collect()
        }
        EventKind::Modify(ModifyKind::Name(_)) => {
            event.paths.into_iter().filter_map(|path| {
                if should_drop_watcher_path(&path) {
                    return None;
                }
                Some(FileChange {
                    kind: FileChangeKind::Modified,
                    fingerprint: build_fingerprint(&path),
                    old_path: None,
                    new_path: None,
                    path,
                })
            }).collect()
        }
        _ => event.paths.into_iter().filter_map(|path| {
            if should_drop_watcher_path(&path) {
                return None;
            }
            Some(FileChange {
                kind: FileChangeKind::Modified,
                fingerprint: build_fingerprint(&path),
                old_path: None,
                new_path: None,
                path,
            })
        }).collect(),
    }
}

fn should_drop_watcher_path(path: &Path) -> bool {
    if should_skip_path(path) {
        return true;
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if matches!(name.as_str(), "query-cache.bin" | "work-products.bin") {
        return true;
    }

    false
}

fn build_fingerprint(path: &Path) -> Option<crate::models::FileFingerprint> {
    if !path.exists() || !path.is_file() {
        return None;
    }
    fingerprint_file(path).ok()
}

fn summarize_changes(changes: &[FileChange]) -> String {
    if changes.len() == 1 {
        let change = &changes[0];
        let label = match change.kind {
            FileChangeKind::Created => "Created",
            FileChangeKind::Modified => "Modified",
            FileChangeKind::Deleted => "Deleted",
            FileChangeKind::Renamed => "Renamed",
        };
        return format!(
            "{}: {}",
            label,
            change.path.file_name().unwrap_or_default().to_string_lossy()
        );
    }

    format!("{} file changes detected", changes.len())
}
