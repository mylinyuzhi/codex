//! Spawn the skill-change watcher and hot-reload the session's skill
//! catalog + slash-command registry on `.md` edits.
//!
//! TS parity: `utils/skills/skillChangeDetector.ts`, registered at
//! startup in `main.tsx` (`void skillChangeDetector.initialize()`).
//!
//! Two layers of reload happen on each debounced burst:
//! 1. [`SkillChangeDetector`] reloads the [`coco_skills::SkillManager`]
//!    catalog **in place** — because `SkillManager` has interior
//!    `RwLock`, the per-turn skill reminder ([`coco_system_reminder`]
//!    `SkillsSource`) and the model-facing `SkillTool` listing pick up
//!    the change with no further plumbing.
//! 2. The forwarder rebuilds the slash-command registry (the coco-rs
//!    equivalent of TS `clearCommandsCache()`) so user-typed
//!    `/skill-name` commands reflect added/removed skills, and pushes
//!    the fresh command list to the TUI's `/` autocomplete.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use coco_skills::watcher::SkillChangeDetector;
use coco_types::CoreEvent;
use coco_types::TuiOnlyEvent;
use tokio::sync::mpsc;

use crate::session_runtime::SessionRuntime;

/// Skill directories watched in every interactive session — the same
/// dirs [`crate::session_runtime::SessionRuntime::reload_plugins`] /
/// `build_session_command_registry` load from:
/// - `<config_home>/skills` — user scope
/// - `<cwd>/.coco/skills` — project scope
pub fn default_watch_paths(cwd: &Path, config_home: &Path) -> Vec<PathBuf> {
    vec![config_home.join("skills"), cwd.join(".coco").join("skills")]
}

/// Spawn the skill-change detector plus a forwarder that rebuilds the
/// slash-command registry and refreshes the TUI command list on each
/// debounced burst.
///
/// Returns the `Arc<SkillChangeDetector>` the caller must hold for the
/// session lifetime (drop = clean shutdown — the wrapped `FileWatcher`
/// and the forwarder task both stop when the last `Arc` drops). Returns
/// `None` when construction fails (logged at `warn`); the session
/// continues without hot-reload rather than aborting.
pub fn spawn(
    runtime: Arc<SessionRuntime>,
    notify_tx: mpsc::Sender<CoreEvent>,
    cwd: PathBuf,
    config_home: PathBuf,
) -> Option<Arc<SkillChangeDetector>> {
    let paths = default_watch_paths(&cwd, &config_home);
    match SkillChangeDetector::new(runtime.skill_manager(), paths) {
        Ok(detector) => {
            let mut rx = detector.subscribe();
            tokio::spawn(async move {
                while rx.recv().await.is_ok() {
                    // The catalog is already reloaded in place by the
                    // detector. Rebuild the slash-command registry from the
                    // fresh on-disk skills (TS `clearCommandsCache()`) and
                    // push the refreshed list to the `/` autocomplete.
                    let count = runtime.reload_plugins(&cwd).await;
                    tracing::info!(commands = count, "skills changed: command registry rebuilt");
                    let snapshot = runtime.current_command_registry().await.snapshot_for_ui();
                    let _ = notify_tx
                        .send(CoreEvent::Tui(TuiOnlyEvent::AvailableCommandsRefreshed {
                            commands: snapshot,
                        }))
                        .await;
                }
            });
            Some(detector)
        }
        Err(err) => {
            tracing::warn!("skill watcher disabled: {err}");
            None
        }
    }
}

#[cfg(test)]
#[path = "skill_watch.test.rs"]
mod tests;
