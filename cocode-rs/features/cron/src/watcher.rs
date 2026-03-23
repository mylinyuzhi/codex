//! File watcher for external changes to scheduled_tasks.json.
//!
//! Detects when another session modifies the durable task file and
//! notifies subscribers to reload.

use std::path::Path;
use std::time::Duration;

use cocode_file_watch::FileWatcher;
use cocode_file_watch::FileWatcherBuilder;
use cocode_file_watch::RecursiveMode;
use tokio::sync::broadcast;

/// Debounce interval for task file changes.
const DEBOUNCE_MS: u64 = 300;

/// Domain event: the scheduled_tasks.json file was modified.
#[derive(Debug, Clone)]
pub struct TaskFileChanged;

/// Watches `scheduled_tasks.json` for external modifications.
pub struct TaskFileWatcher {
    inner: FileWatcher<TaskFileChanged>,
}

impl TaskFileWatcher {
    /// Create a new watcher for the task file in `cocode_home`.
    ///
    /// Returns `None` if the watcher cannot be initialized (e.g., on
    /// unsupported filesystems).
    pub fn new(cocode_home: &Path) -> Option<Self> {
        let watcher: FileWatcher<TaskFileChanged> = FileWatcherBuilder::new()
            .throttle_interval(Duration::from_millis(DEBOUNCE_MS))
            .build(
                |event| {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => Some(TaskFileChanged),
                        _ => None,
                    }
                },
                |_a, b| b, // merge: take the latest
            )
            .map_err(|e| {
                tracing::warn!(error = %e, "Failed to create task file watcher");
            })
            .ok()?;

        let task_file = cocode_home.join("scheduled_tasks.json");
        if task_file.exists() {
            watcher.watch(task_file, RecursiveMode::NonRecursive);
        } else {
            // Watch the directory for the file to be created
            watcher.watch(cocode_home.to_path_buf(), RecursiveMode::NonRecursive);
        }

        Some(Self { inner: watcher })
    }

    /// Subscribe to task file change notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<TaskFileChanged> {
        self.inner.subscribe()
    }
}

#[cfg(test)]
#[path = "watcher.test.rs"]
mod tests;
