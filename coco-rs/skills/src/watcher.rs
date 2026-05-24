//! Skill change detection and dynamic reload.
//!
//! TS: utils/skills/skillChangeDetector.ts — watches skill directories
//! for `.md` file changes, debounces, and triggers skill reload.
//!
//! ## Reuse note
//!
//! Thin wrapper around [`coco_file_watch::FileWatcher`] (mirror
//! [`coco_plugins::watcher::PluginChangeDetector`]). Caller holds the
//! returned `Arc<SkillChangeDetector>` as a guard binding for the
//! session lifetime; dropping the Arc shuts the watcher down cleanly.

use coco_file_watch::FileWatcher;
use coco_file_watch::FileWatcherBuilder;
use coco_file_watch::RecursiveMode;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::SkillDirFormat;
use crate::SkillManager;
use crate::discover_skills_with_format;

/// Debounce interval for skill file changes (matches TS 300ms).
const SKILL_DEBOUNCE_MS: u64 = 300;

/// Event emitted when skill files change.
#[derive(Debug, Clone, Default)]
pub struct SkillsChanged {
    /// Paths of files that changed (for diagnostics).
    pub changed_paths: Vec<PathBuf>,
    /// Hook declarations from the reloaded skill set:
    /// `(skill_name, hooks_json)`. Populated post-reload so subscribers
    /// (the hooks layer) can re-register skill-declared hooks without a
    /// session restart.
    ///
    /// TS: `skillChangeDetector.ts` triggers both skill reload AND
    /// `hooksConfigSnapshot` invalidation. In Rust the snapshot refresh
    /// is the subscriber's responsibility — this field carries the
    /// minimum data needed (opaque hook JSON) for coco-hooks to rebuild
    /// its skill-scope entries.
    pub skill_hook_declarations: Vec<(String, serde_json::Value)>,
}

/// Watches skill directories and reloads the [`SkillManager`] on
/// changes.
///
/// TS: SkillChangeDetector — debounced file watcher that triggers skill
/// reload.
///
/// ## Lifecycle
///
/// The watcher's background tasks live inside the wrapped
/// [`FileWatcher`]; when the last `Arc<SkillChangeDetector>` is dropped,
/// the watcher (and its notify thread) drops with it. Callers hold the
/// `Arc` in a guard binding for the session lifetime.
pub struct SkillChangeDetector {
    /// Wrapped generic watcher — owns the OS-event pump, throttle
    /// timer, and post-debounce broadcast channel. The change-tx below
    /// is downstream of this: a tokio task lifts each debounced
    /// `SkillsChanged` into a reload step then re-broadcasts so
    /// subscribers see the enriched form (`skill_hook_declarations`
    /// populated).
    _inner: FileWatcher<SkillsChanged>,
    /// Shared skill manager that gets reloaded on changes.
    manager: Arc<SkillManager>,
    /// Directories being watched.
    watched_dirs: Vec<PathBuf>,
    /// Broadcast sender for change notifications (post-reload —
    /// `skill_hook_declarations` is filled in).
    change_tx: broadcast::Sender<SkillsChanged>,
}

impl SkillChangeDetector {
    /// Create a new detector watching the given directories.
    ///
    /// The detector immediately subscribes to filesystem events and
    /// will reload the [`SkillManager`] when `.md` files change.
    /// Returns an `Arc<Self>` so the caller can hold a guard binding
    /// to control lifetime.
    pub fn new(manager: Arc<SkillManager>, skill_dirs: Vec<PathBuf>) -> crate::Result<Arc<Self>> {
        let inner = FileWatcherBuilder::new()
            .throttle_interval(Duration::from_millis(SKILL_DEBOUNCE_MS))
            .build(classify, merge)
            .map_err(|e| crate::SkillsError::generic(format!("file-watch build failed: {e}")))?;

        for dir in &skill_dirs {
            // `try_watch` already returns Ok(()) for non-existent paths
            // — no pre-check needed.
            inner.watch(dir.clone(), RecursiveMode::Recursive);
        }

        // Bridge: each debounced `SkillsChanged` from the FileWatcher
        // triggers a fresh skill scan, then we re-broadcast on
        // `change_tx` with hook declarations populated. Subscribers
        // (coco-hooks) need that enriched payload.
        let (change_tx, _) = broadcast::channel(32);
        let change_tx_clone = change_tx.clone();
        let manager_clone = Arc::clone(&manager);
        let dirs_clone = skill_dirs.clone();
        let mut rx = inner.subscribe();
        tokio::spawn(async move {
            while let Ok(mut event) = rx.recv().await {
                tracing::info!(
                    paths = ?event.changed_paths,
                    "skill files changed, reloading"
                );
                let new_skills: Vec<_> = dirs_clone
                    .iter()
                    .flat_map(|dir| {
                        let format = if dir.ends_with("commands") {
                            SkillDirFormat::Legacy
                        } else {
                            SkillDirFormat::SkillMdOnly
                        };
                        discover_skills_with_format(std::slice::from_ref(dir), format)
                    })
                    .collect();

                event.skill_hook_declarations = new_skills
                    .iter()
                    .filter_map(|s| s.hooks.as_ref().map(|h| (s.name.clone(), h.clone())))
                    .collect();

                // Interior mutability — no Mutex needed since
                // SkillManager has internal RwLock.
                manager_clone.reload_disk_skills(new_skills);
                // Re-register bundled skills so they're always
                // available (TS check on `process.env.USER_TYPE` →
                // `UserType::from_env()`).
                crate::bundled::register_bundled(&manager_clone, coco_types::UserType::from_env());
                tracing::info!(count = manager_clone.len(), "skills reloaded");

                let _ = change_tx_clone.send(event);
            }
        });

        Ok(Arc::new(SkillChangeDetector {
            _inner: inner,
            manager,
            watched_dirs: skill_dirs,
            change_tx,
        }))
    }

    /// Subscribe to skill change notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<SkillsChanged> {
        self.change_tx.subscribe()
    }

    /// Get a reference to the managed [`SkillManager`].
    pub fn manager(&self) -> &Arc<SkillManager> {
        &self.manager
    }

    /// Get the watched directories.
    pub fn watched_dirs(&self) -> &[PathBuf] {
        &self.watched_dirs
    }
}

// ─── classify + merge (testable, extracted from build()) ────────────────

fn classify(event: &coco_file_watch::Event) -> Option<SkillsChanged> {
    let md_paths: Vec<PathBuf> = event
        .paths
        .iter()
        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
        .cloned()
        .collect();
    if md_paths.is_empty() {
        return None;
    }
    Some(SkillsChanged {
        changed_paths: md_paths,
        skill_hook_declarations: Vec::new(),
    })
}

fn merge(mut acc: SkillsChanged, new: SkillsChanged) -> SkillsChanged {
    acc.changed_paths.extend(new.changed_paths);
    // `skill_hook_declarations` is populated post-reload by the bridge
    // task, so merging two raw classify outputs always leaves it empty.
    acc
}
