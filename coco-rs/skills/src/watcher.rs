//! Skill change detection and dynamic reload.
//!
//! TS: utils/skills/skillChangeDetector.ts — watches skill directories
//! for .md file changes, debounces, and triggers skill reload.

use coco_file_watch::FileWatcherBuilder;
use coco_file_watch::RecursiveMode;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::broadcast;

use crate::SkillDefinition;
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
    /// Hook declarations from the reloaded skill set: `(skill_name, hooks_json)`.
    /// Populated post-reload so subscribers (the hooks layer) can re-register
    /// skill-declared hooks without a session restart.
    ///
    /// TS: `skillChangeDetector.ts` triggers both skill reload AND
    /// `hooksConfigSnapshot` invalidation. In Rust the snapshot refresh is the
    /// subscriber's responsibility — this field carries the minimum data
    /// needed (opaque hook JSON) for coco-hooks to rebuild its skill-scope
    /// entries.
    pub skill_hook_declarations: Vec<(String, serde_json::Value)>,
}

/// Watches skill directories and reloads the SkillManager on changes.
///
/// TS: SkillChangeDetector — debounced file watcher that triggers skill reload.
pub struct SkillChangeDetector {
    /// Shared skill manager that gets reloaded on changes.
    manager: Arc<Mutex<SkillManager>>,
    /// Directories being watched.
    watched_dirs: Vec<PathBuf>,
    /// Broadcast sender for change notifications.
    change_tx: broadcast::Sender<SkillsChanged>,
}

impl SkillChangeDetector {
    /// Create a new detector watching the given directories.
    ///
    /// The detector immediately subscribes to filesystem events and will
    /// reload the SkillManager when .md files change.
    pub fn new(
        manager: Arc<Mutex<SkillManager>>,
        skill_dirs: Vec<PathBuf>,
    ) -> anyhow::Result<Self> {
        let (change_tx, _) = broadcast::channel(32);
        let change_tx_clone = change_tx.clone();
        let manager_clone = Arc::clone(&manager);
        let dirs_clone = skill_dirs.clone();

        let watcher = FileWatcherBuilder::new()
            .throttle_interval(Duration::from_millis(SKILL_DEBOUNCE_MS))
            .build(
                |event| {
                    // Only care about .md files
                    let md_paths: Vec<PathBuf> = event
                        .paths
                        .iter()
                        .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
                        .cloned()
                        .collect();
                    if md_paths.is_empty() {
                        None
                    } else {
                        Some(SkillsChanged {
                            changed_paths: md_paths,
                            skill_hook_declarations: Vec::new(),
                        })
                    }
                },
                |mut acc, new| {
                    acc.changed_paths.extend(new.changed_paths);
                    acc
                },
            )?;

        // Watch all skill directories recursively
        for dir in &skill_dirs {
            if dir.exists() {
                watcher.watch(dir.clone(), RecursiveMode::Recursive);
                tracing::debug!("watching skill directory: {}", dir.display());
            }
        }

        // Spawn background task to handle change events
        let mut rx = watcher.subscribe();
        tokio::spawn(async move {
            while let Ok(mut event) = rx.recv().await {
                tracing::info!(
                    paths = ?event.changed_paths,
                    "skill files changed, reloading"
                );

                // Reload all skills from directories
                // Use SkillMdOnly for skills dirs, Legacy for commands dirs
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

                // Harvest hook declarations from the reloaded set so downstream
                // hooks infrastructure can refresh its skill-scope entries.
                event.skill_hook_declarations = new_skills
                    .iter()
                    .filter_map(|s| s.hooks.as_ref().map(|h| (s.name.clone(), h.clone())))
                    .collect();

                let mut mgr = manager_clone.lock().await;
                reload_manager(&mut mgr, new_skills);
                drop(mgr); // release before broadcasting so subscribers don't deadlock

                // Notify subscribers (hooks layer consumes skill_hook_declarations).
                let _ = change_tx_clone.send(event);
            }
        });

        // Keep watcher alive by leaking it (it's a background process)
        // The watcher will be dropped when the process exits
        std::mem::forget(watcher);

        Ok(SkillChangeDetector {
            manager,
            watched_dirs: skill_dirs,
            change_tx,
        })
    }

    /// Subscribe to skill change notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<SkillsChanged> {
        self.change_tx.subscribe()
    }

    /// Get a reference to the managed SkillManager.
    pub fn manager(&self) -> &Arc<Mutex<SkillManager>> {
        &self.manager
    }

    /// Get the watched directories.
    pub fn watched_dirs(&self) -> &[PathBuf] {
        &self.watched_dirs
    }
}

/// Replace all skills in a manager with newly discovered ones.
fn reload_manager(manager: &mut SkillManager, skills: Vec<SkillDefinition>) {
    *manager = SkillManager::new();
    for skill in skills {
        manager.register(skill);
    }
    // Re-register bundled skills so they're always available
    crate::bundled::register_bundled(manager);
    tracing::info!(count = manager.len(), "skills reloaded");
}
