use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use coco_file_watch::Event as FsEvent;
use coco_file_watch::FileWatcher;
use coco_file_watch::FileWatcherBuilder;
use coco_file_watch::RecursiveMode;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::mpsc;
use tracing::warn;

use super::ThemeLoadResult;
use super::load_theme_runtime_or_default;
use super::theme_config_path;

const THEME_RELOAD_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
struct ThemeConfigChanged;

pub struct ThemeWatcher {
    watcher: FileWatcher<ThemeConfigChanged>,
}

impl ThemeWatcher {
    pub fn watch_default() -> Result<Self> {
        Self::watch_path(theme_config_path())
    }

    pub fn watch_path(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let original = path.clone();
        let canonical = canonical_key(&path);
        let watcher = FileWatcherBuilder::<ThemeConfigChanged>::new()
            .throttle_interval(THEME_RELOAD_DEBOUNCE)
            .build(
                move |event: &FsEvent| {
                    event
                        .paths
                        .iter()
                        .any(|path| path == &original || path == &canonical)
                        .then_some(ThemeConfigChanged)
                },
                |_old, new| new,
            )?;

        if let Some(parent) = path.parent() {
            watcher
                .try_watch(parent.to_path_buf(), RecursiveMode::NonRecursive)
                .with_context(|| format!("failed to watch {}", parent.display()))?;
        }

        Ok(Self { watcher })
    }

    fn noop() -> Self {
        Self {
            watcher: FileWatcherBuilder::<ThemeConfigChanged>::new().build_noop(),
        }
    }

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ThemeConfigChanged> {
        self.watcher.subscribe()
    }
}

pub struct ThemeSetup {
    pub watcher: ThemeWatcher,
    pub reload_rx: mpsc::Receiver<ThemeLoadResult>,
    pub initial: ThemeLoadResult,
    pub watch_error: Option<String>,
}

pub async fn install_theme() -> ThemeSetup {
    let (watcher, watch_error) = match ThemeWatcher::watch_default() {
        Ok(watcher) => (watcher, None),
        Err(err) => {
            warn!(error = %err, "theme hot reload disabled");
            (
                ThemeWatcher::noop(),
                Some(format!("Theme hot reload disabled: {err}")),
            )
        }
    };
    let mut watch_rx = watcher.subscribe();
    let initial = load_theme_runtime_or_default();
    let (reload_tx, reload_rx) = mpsc::channel::<ThemeLoadResult>(8);
    tokio::spawn(async move {
        loop {
            match watch_rx.recv().await {
                Ok(_) => {
                    if reload_tx
                        .send(load_theme_runtime_or_default())
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });

    ThemeSetup {
        watcher,
        reload_rx,
        initial,
        watch_error,
    }
}

fn canonical_key(path: &Path) -> PathBuf {
    path.parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .and_then(|parent| path.file_name().map(|name| parent.join(name)))
        .unwrap_or_else(|| path.to_path_buf())
}
