//! Skill change detection and dynamic reload.
//!
//! TS: utils/skills/skillChangeDetector.ts — watches skill directories
//! for `.md` file changes, debounces, and triggers skill reload.
//!
//! ## TS chokidar parity
//!
//! - `stability_threshold` (default 1000ms): wait this long after the
//!   last write for a path before treating it as settled. Mirrors
//!   chokidar `awaitWriteFinish.stabilityThreshold`. Editors / tools
//!   that write in bursts (atomic-rename, multiple `write()` calls)
//!   would otherwise trigger N reloads.
//! - `poll_interval` (default 2000ms): how often the stability poller
//!   sweeps the pending-events map. Mirrors chokidar `interval`
//!   (used as `awaitWriteFinish.pollInterval` and as the filesystem
//!   poll interval when `usePolling: true` is on under Bun).
//! - `debounce` (default 300ms): coalesce multiple settled events
//!   into a single reload. Mirrors `RELOAD_DEBOUNCE_MS`.
//! - `ignored_dirs` (default `[".git"]`): path-component names to
//!   filter out. Mirrors the chokidar `ignored` predicate that
//!   discards `.git` traffic.
//!
//! ## Reuse note
//!
//! Thin wrapper around [`coco_file_watch::FileWatcher`] (mirror
//! [`coco_plugins::watcher::PluginChangeDetector`]). Caller holds the
//! returned `Arc<SkillChangeDetector>` as a guard binding for the
//! session lifetime; dropping the Arc shuts the watcher down cleanly.

use async_trait::async_trait;
use coco_file_watch::FileWatcher;
use coco_file_watch::FileWatcherBuilder;
use coco_file_watch::RecursiveMode;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::SkillDirFormat;
use crate::SkillManager;
use crate::discover_skills_with_format;

/// Default stability threshold (TS `FILE_STABILITY_THRESHOLD_MS = 1000`).
pub const DEFAULT_STABILITY_THRESHOLD: Duration = Duration::from_millis(1000);
/// Default stability poll interval (TS `POLLING_INTERVAL_MS = 2000` —
/// also reused as `awaitWriteFinish.pollInterval`).
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(2000);
/// Default reload-coalesce window (TS `RELOAD_DEBOUNCE_MS = 300`).
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(300);

/// Configuration knobs for the skills watcher. Mirrors the four TS
/// chokidar knobs (`stabilityThreshold`, `pollInterval`,
/// `RELOAD_DEBOUNCE_MS`, `ignored`).
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// How long after the last write for a path before treating it as
    /// settled. TS `awaitWriteFinish.stabilityThreshold = 1000ms`.
    pub stability_threshold: Duration,
    /// Stability poller sweep interval. TS `interval = 2000ms`.
    pub poll_interval: Duration,
    /// Debounce window — coalesce stable events within this window
    /// into a single reload. TS `RELOAD_DEBOUNCE_MS = 300ms`.
    pub debounce: Duration,
    /// Directory component names to skip. TS hard-codes `.git`; we
    /// keep the list configurable so callers can opt into more.
    pub ignored_dirs: Vec<PathBuf>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            stability_threshold: DEFAULT_STABILITY_THRESHOLD,
            poll_interval: DEFAULT_POLL_INTERVAL,
            debounce: DEFAULT_DEBOUNCE,
            ignored_dirs: vec![PathBuf::from(".git")],
        }
    }
}

/// Callback the watcher fires before reloading skills, mirroring TS
/// `executeConfigChangeHooks('skills', path)` (`skillChangeDetector.ts:267`).
///
/// Implementations live outside this crate (typically `coco-query` /
/// session runtime), where the full `coco_hooks::OrchestrationContext`
/// is reachable. Returning `Ok(true)` blocks the reload (matches TS
/// `hasBlockingResult(results)`); returning `Ok(false)` or `Err(_)`
/// allows the reload to proceed (errors are logged at `warn`).
///
/// The path passed is one representative member of the debounced
/// batch — TS deliberately fires a single hook per batch rather than
/// per-path so hook matchers don't see N duplicate queries.
#[async_trait]
pub trait ConfigChangeHookDispatcher: Send + Sync {
    /// Fire `ConfigChange` hooks for the `skills` source. Return
    /// `Ok(true)` to block the reload.
    async fn dispatch_skills_change(&self, representative_path: &Path) -> Result<bool, String>;
}

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
    /// `true` when a ConfigChange hook returned blocking — the reload
    /// was skipped. TS parity: `hasBlockingResult(results)` short-circuit
    /// in `skillChangeDetector.ts:268-271`.
    pub blocked_by_hook: bool,
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
    _inner: FileWatcher<RawSkillsEvent>,
    /// Shared skill manager that gets reloaded on changes.
    manager: Arc<SkillManager>,
    /// Directories being watched.
    watched_dirs: Vec<PathBuf>,
    /// Broadcast sender for change notifications (post-reload —
    /// `skill_hook_declarations` is filled in).
    change_tx: broadcast::Sender<SkillsChanged>,
}

/// Internal event carried from the file-watcher into the stability
/// poller. Distinct from the public [`SkillsChanged`] because we don't
/// know the hook declarations yet — those come after the reload.
#[derive(Debug, Clone, Default)]
struct RawSkillsEvent {
    paths: Vec<PathBuf>,
}

impl SkillChangeDetector {
    /// Create a new detector watching the given directories with
    /// default [`WatcherConfig`] and no hook dispatcher.
    ///
    /// The detector immediately subscribes to filesystem events and
    /// will reload the [`SkillManager`] when `.md` files change.
    /// Returns an `Arc<Self>` so the caller can hold a guard binding
    /// to control lifetime.
    pub fn new(manager: Arc<SkillManager>, skill_dirs: Vec<PathBuf>) -> crate::Result<Arc<Self>> {
        Self::with_config(
            manager,
            skill_dirs,
            Vec::new(),
            WatcherConfig::default(),
            None,
        )
    }

    /// Create a new detector with custom config, additional watch
    /// roots, and an optional ConfigChange hook dispatcher.
    ///
    /// `additional_dirs` are pre-resolved skill directories from
    /// `RuntimeConfig.permissions.additional_directories` joined with
    /// `.claude/skills`. Callers compute the join themselves so this
    /// crate doesn't have to know about `RuntimeConfig` layering. TS:
    /// `getAdditionalDirectoriesForClaudeMd()` in `bootstrap/state.ts`
    /// joined with `.claude/skills` (`skillChangeDetector.ts:224-232`).
    pub fn with_config(
        manager: Arc<SkillManager>,
        skill_dirs: Vec<PathBuf>,
        additional_dirs: Vec<PathBuf>,
        config: WatcherConfig,
        hook_dispatcher: Option<Arc<dyn ConfigChangeHookDispatcher>>,
    ) -> crate::Result<Arc<Self>> {
        let WatcherConfig {
            stability_threshold,
            poll_interval,
            debounce,
            ignored_dirs,
        } = config;

        // Build the underlying watcher with throttle = debounce. The
        // file-watch crate's throttle handles event coalescing inside
        // the debounce window; the stability check happens on top in
        // the bridge task (cf. doc on `WatcherConfig`).
        let classify =
            move |event: &coco_file_watch::Event| classify_skill_event(event, &ignored_dirs);
        let merge = |mut acc: RawSkillsEvent, new: RawSkillsEvent| {
            acc.paths.extend(new.paths);
            acc
        };

        let inner = FileWatcherBuilder::new()
            .throttle_interval(debounce)
            .build(classify, merge)
            .map_err(|e| crate::SkillsError::generic(format!("file-watch build failed: {e}")))?;

        // Combine skill roots + additional dirs into the watch set.
        let mut all_dirs: Vec<PathBuf> = skill_dirs;
        for dir in additional_dirs {
            if !all_dirs.contains(&dir) {
                all_dirs.push(dir);
            }
        }

        for dir in &all_dirs {
            // `try_watch` already returns Ok(()) for non-existent paths
            // — no pre-check needed.
            inner.watch(dir.clone(), RecursiveMode::Recursive);
        }

        let (change_tx, _) = broadcast::channel(32);
        spawn_stability_bridge(StabilityBridge {
            rx: inner.subscribe(),
            change_tx: change_tx.clone(),
            manager: Arc::clone(&manager),
            skill_dirs: all_dirs.clone(),
            hook_dispatcher,
            stability_threshold,
            poll_interval,
        });

        Ok(Arc::new(SkillChangeDetector {
            _inner: inner,
            manager,
            watched_dirs: all_dirs,
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

    /// Get the watched directories (skill roots + additional dirs).
    pub fn watched_dirs(&self) -> &[PathBuf] {
        &self.watched_dirs
    }
}

// ─── classify (testable, extracted from build()) ────────────────────────

/// Classify raw notify events into [`RawSkillsEvent`].
///
/// - keep only `.md` paths (skills + commands are markdown);
/// - drop any path that traverses an ignored directory component
///   (e.g. `.git/HEAD` when `ignored_dirs` contains `.git`).
fn classify_skill_event(
    event: &coco_file_watch::Event,
    ignored_dirs: &[PathBuf],
) -> Option<RawSkillsEvent> {
    let paths: Vec<PathBuf> = event
        .paths
        .iter()
        .filter(|p| {
            p.extension().is_some_and(|ext| ext == "md") && !path_is_in_ignored_dir(p, ignored_dirs)
        })
        .cloned()
        .collect();
    if paths.is_empty() {
        return None;
    }
    Some(RawSkillsEvent { paths })
}

/// True iff any component of `path` matches any entry in `ignored_dirs`.
///
/// Comparison is by `Component::Normal` against each ignored entry's
/// file_name — so `vec![".git"]` matches `/a/.git/HEAD` regardless of
/// whether the user passed `.git` or `PathBuf::from(".git")`.
fn path_is_in_ignored_dir(path: &Path, ignored_dirs: &[PathBuf]) -> bool {
    let ignored_names: Vec<&OsStr> = ignored_dirs
        .iter()
        .filter_map(|p| {
            p.file_name()
                .or_else(|| p.as_os_str().to_str().map(OsStr::new))
        })
        .collect();
    path.components().any(|c| match c {
        Component::Normal(name) => ignored_names.contains(&name),
        _ => false,
    })
}

// ─── stability bridge ───────────────────────────────────────────────────

struct StabilityBridge {
    rx: broadcast::Receiver<RawSkillsEvent>,
    change_tx: broadcast::Sender<SkillsChanged>,
    manager: Arc<SkillManager>,
    skill_dirs: Vec<PathBuf>,
    hook_dispatcher: Option<Arc<dyn ConfigChangeHookDispatcher>>,
    stability_threshold: Duration,
    poll_interval: Duration,
}

/// Spawn the bridge task that:
///
/// 1. Receives raw debounced `RawSkillsEvent` from the file watcher.
/// 2. Tracks per-path "last-event-time" — drops the path from the
///    pending set until at least `stability_threshold` ms have passed
///    since the most recent event (TS chokidar `awaitWriteFinish`).
/// 3. On every `poll_interval` tick, scans the pending set and forwards
///    settled paths.
/// 4. Fires the ConfigChange hook once per batch (representative path).
///    If the hook returns blocking, skip the reload and surface
///    `blocked_by_hook = true` on the broadcast.
/// 5. Scans skill directories, reloads the manager, broadcasts
///    `SkillsChanged` with `skill_hook_declarations` populated.
fn spawn_stability_bridge(mut bridge: StabilityBridge) {
    tokio::spawn(async move {
        // Per-path "last-event-time" map. Single-task ownership — no
        // lock needed.
        let mut pending: HashMap<PathBuf, Instant> = HashMap::new();
        let mut ticker = tokio::time::interval(bridge.poll_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // First tick fires immediately by default — burn it so we
        // don't sweep an empty map and skew the first stability check.
        ticker.tick().await;

        loop {
            tokio::select! {
                recv = bridge.rx.recv() => {
                    match recv {
                        Ok(raw) => {
                            let now = Instant::now();
                            for p in raw.paths {
                                pending.insert(p, now);
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(
                                "skills watcher lagged behind by {n} events; some changes may be missed"
                            );
                            continue;
                        }
                    }
                }
                _ = ticker.tick() => {
                    let now = Instant::now();
                    let mut settled: Vec<PathBuf> = Vec::new();
                    pending.retain(|p, t| {
                        if now.duration_since(*t) >= bridge.stability_threshold {
                            settled.push(p.clone());
                            false
                        } else {
                            true
                        }
                    });
                    if settled.is_empty() {
                        continue;
                    }
                    handle_settled_batch(&mut bridge, settled).await;
                }
            }
        }
    });
}

/// Process a batch of settled paths: fire ConfigChange hook,
/// reload the manager (unless blocked), broadcast the result.
async fn handle_settled_batch(bridge: &mut StabilityBridge, settled: Vec<PathBuf>) {
    tracing::info!(
        paths = ?settled,
        "skill files settled, reloading"
    );

    // Fire ConfigChange hook once for the batch with a representative
    // path (TS parity: `executeConfigChangeHooks('skills', paths[0])`).
    let mut blocked_by_hook = false;
    if let Some(dispatcher) = &bridge.hook_dispatcher {
        let representative = settled
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(""));
        match dispatcher.dispatch_skills_change(&representative).await {
            Ok(true) => {
                tracing::info!(
                    "ConfigChange hook blocked skill reload ({} paths)",
                    settled.len()
                );
                blocked_by_hook = true;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!("ConfigChange hook for skills failed: {e}");
            }
        }
    }

    let mut event = SkillsChanged {
        changed_paths: settled,
        skill_hook_declarations: Vec::new(),
        blocked_by_hook,
    };

    if !blocked_by_hook {
        let new_skills: Vec<_> = bridge
            .skill_dirs
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
        bridge.manager.reload_disk_skills(new_skills);
        // Re-register bundled skills so they're always
        // available (TS check on `process.env.USER_TYPE` →
        // `UserType::from_env()`).
        crate::bundled::register_bundled(&bridge.manager, coco_types::UserType::from_env());
        tracing::info!(count = bridge.manager.len(), "skills reloaded");
    }

    let _ = bridge.change_tx.send(event);
}

#[cfg(test)]
#[path = "watcher.test.rs"]
mod tests;
