//! Watches `.git/index` to invalidate the file index after commits and
//! checkouts.
//!
//! TS: `fileSuggestions.ts:142,738` — the TS implementation polls the git
//! index mtime each keystroke and re-fires the search on change. The Rust
//! port uses `coco_file_watch::FileWatcher` over the `notify` crate so the
//! refresh runs on actual filesystem events, not on every keystroke.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use coco_file_search::FileIndex;
use coco_file_search::SharedFileIndex;
use coco_file_watch::FileWatcherBuilder;
use coco_file_watch::RecursiveMode;
use tracing::debug;

/// Throttle window — coalesce bursts when git rewrites `index` (commit,
/// checkout, rebase). 500 ms is short enough to feel live, long enough
/// to dedupe a write+rename pair.
const THROTTLE: Duration = Duration::from_millis(500);

/// Spawn a background watcher that triggers `FileIndex::refresh_background`
/// whenever `.git/index` (or the broader `.git/` dir, when `index` is
/// rewritten via rename) changes.
///
/// No-op if `cwd` has no `.git/` directory.
pub fn spawn(cwd: PathBuf, index: SharedFileIndex) {
    let git_dir = cwd.join(".git");
    if !git_dir.is_dir() {
        debug!("git_index_watcher: no .git/ at {cwd:?}, skipping");
        return;
    }

    // Domain event: at least one path under `.git/` changed. We don't
    // need to inspect which one — any rewrite of the index implies the
    // tracked-file list may have shifted.
    #[derive(Debug, Clone)]
    struct GitChange;

    let watcher = match FileWatcherBuilder::<GitChange>::new()
        .throttle_interval(THROTTLE)
        .build(
            |event| {
                if event.paths.iter().any(|p| matches_git_index(p.as_path())) {
                    Some(GitChange)
                } else {
                    None
                }
            },
            |acc, _| acc,
        ) {
        Ok(w) => w,
        Err(e) => {
            debug!("git_index_watcher: failed to build watcher: {e}");
            return;
        }
    };

    watcher.watch(git_dir, RecursiveMode::NonRecursive);
    let mut rx = watcher.subscribe();
    tokio::spawn(async move {
        // Hold the watcher alive for the lifetime of the task.
        let _watcher = watcher;
        while rx.recv().await.is_ok() {
            FileIndex::refresh_background(index.clone());
        }
    });
}

fn matches_git_index(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n == "index" || n == "HEAD" || n == "ORIG_HEAD")
}
