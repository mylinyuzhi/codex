//! Keybinding file watcher for hot-reload.
//!
//! Uses `cocode-file-watch` to watch `keybindings.json` for changes and
//! reload bindings when the file is modified.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use cocode_file_watch::FileWatcher;
use cocode_file_watch::FileWatcherBuilder;
use cocode_file_watch::RecursiveMode;
use tracing::info;

/// Domain event for keybinding file changes.
#[derive(Debug, Clone)]
pub struct KeybindingsChanged {
    pub paths: Vec<PathBuf>,
}

/// Throttle interval for keybinding file changes (500ms, matching Claude Code).
const THROTTLE_INTERVAL: Duration = Duration::from_millis(500);

/// Create a file watcher for `keybindings.json`.
///
/// Returns a watcher that emits `KeybindingsChanged` events when the
/// file is modified. The caller should subscribe and reload bindings.
pub fn create_watcher() -> Result<FileWatcher<KeybindingsChanged>, notify::Error> {
    FileWatcherBuilder::new()
        .throttle_interval(THROTTLE_INTERVAL)
        .build(
            |event| {
                use notify::EventKind;
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) => {
                        if event.paths.is_empty() {
                            None
                        } else {
                            Some(KeybindingsChanged {
                                paths: event.paths.clone(),
                            })
                        }
                    }
                    _ => None,
                }
            },
            |mut acc, new| {
                acc.paths.extend(new.paths);
                acc
            },
        )
}

/// Create a no-op watcher (for when customization is disabled or in tests).
pub fn create_noop_watcher() -> FileWatcher<KeybindingsChanged> {
    FileWatcherBuilder::new().build_noop()
}

/// Start watching the keybindings file in the given config directory.
pub fn watch_keybindings_file(watcher: &FileWatcher<KeybindingsChanged>, config_dir: &Path) {
    let path = crate::loader::keybindings_file_path(config_dir);
    // Watch the parent directory (to detect file creation).
    if let Some(parent) = path.parent() {
        info!("watching {} for keybinding changes", parent.display());
        watcher.watch(parent.to_path_buf(), RecursiveMode::NonRecursive);
    }
}

#[cfg(test)]
#[path = "watcher.test.rs"]
mod tests;
