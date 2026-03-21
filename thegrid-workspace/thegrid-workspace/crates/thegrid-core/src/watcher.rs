use anyhow::Result;
use notify_debouncer_mini::{
    new_debouncer, DebounceEventResult, Debouncer,
};
use notify::RecommendedWatcher;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use crate::events::AppEvent;

/// Watches filesystem paths and emits `AppEvent::FileSystemChanged`
pub struct FileWatcher {
    _debouncer: Debouncer<RecommendedWatcher>,
    tx: mpsc::Sender<AppEvent>,
}

impl FileWatcher {
    pub fn new(event_tx: mpsc::Sender<AppEvent>) -> Result<Self> {
        let tx_clone = event_tx.clone();

        let debouncer = new_debouncer(
            Duration::from_millis(500),
            move |result: DebounceEventResult| {
                match result {
                    Ok(events) => {
                        let mut paths: Vec<PathBuf> = events
                            .into_iter()
                            .map(|e| e.path)
                            .collect();
                        paths.sort();
                        paths.dedup();

                        if paths.is_empty() { return; }

                        let summary = if paths.len() == 1 {
                            format!(
                                "Changed: {}",
                                paths[0]
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                            )
                        } else {
                            format!("{} files changed", paths.len())
                        };

                        log::debug!("FileWatcher: {}", summary);
                        let _ = tx_clone.send(AppEvent::FileSystemChanged { paths, summary });
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        log::error!("FileWatcher error: {}", msg);
                        let _ = tx_clone.send(AppEvent::FileWatcherError(msg));
                    }
                }
            }
        ).map_err(|e| anyhow::anyhow!("Creating file watcher: {}", e))?;

        Ok(Self { _debouncer: debouncer, tx: event_tx })
    }

    pub fn watch(&mut self, path: PathBuf) -> Result<()> {
        self._debouncer
            .watcher()
            .watch(&path, notify::RecursiveMode::Recursive)
            .map_err(|e| anyhow::anyhow!("Watching {:?}: {}", path, e))?;

        log::info!("FileWatcher: now watching {:?}", path);
        let _ = self.tx.send(AppEvent::Status(
            format!("Watching: {}", path.display())
        ));
        Ok(())
    }

    pub fn unwatch(&mut self, path: &PathBuf) -> Result<()> {
        self._debouncer
            .watcher()
            .unwatch(path)
            .map_err(|e| anyhow::anyhow!("Unwatching {:?}: {}", path, e))?;
        log::info!("FileWatcher: stopped watching {:?}", path);
        Ok(())
    }
}
