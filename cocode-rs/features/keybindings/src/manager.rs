//! Keybinding manager — main entry point.
//!
//! Owns the binding resolver, chord matcher, and file watcher. Provides
//! the primary API for the TUI to process key events.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use crossterm::event::KeyEvent;
use tokio::sync::broadcast;
use tracing::info;
use tracing::warn;

use crate::action::Action;
use crate::chord::ChordMatcher;
use crate::chord::ChordResult;
use crate::context::KeybindingContext;
use crate::defaults::default_bindings;
use crate::loader::load_user_bindings;
use crate::merge::merge_bindings;
use crate::resolver::BindingResolver;
use crate::watcher::KeybindingsChanged;
use crate::watcher::create_noop_watcher;
use crate::watcher::create_watcher;
use crate::watcher::watch_keybindings_file;

/// Result of processing a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeybindingResult {
    /// A keybinding matched — execute this action.
    Action(Action),
    /// A chord prefix was detected — waiting for more keys.
    PendingChord,
    /// A pending chord was cancelled (by Escape or timeout).
    ChordCancelled,
    /// No binding matched — fall through to default handling.
    Unhandled,
}

/// Central keybinding manager.
///
/// Uses `std::sync::Mutex` (not `tokio::sync::Mutex`) for the chord matcher
/// because the critical section is very short (microseconds, no I/O or .await)
/// and this avoids the overhead of an async mutex.
pub struct KeybindingsManager {
    resolver: Arc<RwLock<BindingResolver>>,
    chord: std::sync::Mutex<ChordMatcher>,
    _watcher: cocode_file_watch::FileWatcher<KeybindingsChanged>,
}

impl KeybindingsManager {
    /// Create a new manager with default bindings and optional user overrides.
    ///
    /// If `customization_enabled` is false, user config is not loaded and
    /// the file watcher is not started.
    pub fn new(config_dir: PathBuf, customization_enabled: bool) -> Self {
        let defaults = default_bindings();

        let (user_bindings, warnings) = if customization_enabled {
            load_user_bindings(&config_dir)
        } else {
            (Vec::new(), Vec::new())
        };

        for w in &warnings {
            warn!("keybinding: {w}");
        }

        let merged = merge_bindings(defaults, user_bindings);
        let resolver = Arc::new(RwLock::new(BindingResolver::new(merged)));

        let watcher = if customization_enabled {
            match create_watcher() {
                Ok(w) => {
                    watch_keybindings_file(&w, &config_dir);
                    Self::spawn_reload_task(w.subscribe(), Arc::clone(&resolver), config_dir);
                    w
                }
                Err(err) => {
                    warn!("failed to create keybinding file watcher: {err}");
                    create_noop_watcher()
                }
            }
        } else {
            create_noop_watcher()
        };

        Self {
            resolver,
            chord: std::sync::Mutex::new(ChordMatcher::new()),
            _watcher: watcher,
        }
    }

    /// Create a manager with only default bindings (no file, no watcher).
    pub fn defaults_only() -> Self {
        let resolver = Arc::new(RwLock::new(BindingResolver::new(default_bindings())));
        Self {
            resolver,
            chord: std::sync::Mutex::new(ChordMatcher::new()),
            _watcher: create_noop_watcher(),
        }
    }

    /// Process a key event and return the result.
    ///
    /// Handles chord detection and resolution. The caller should map
    /// `KeybindingResult::Action` to the appropriate `TuiCommand`.
    pub fn process_key(
        &self,
        active_contexts: &[KeybindingContext],
        event: &KeyEvent,
    ) -> KeybindingResult {
        let resolver = self
            .resolver
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut chord = self
            .chord
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if !chord.is_pending() && !resolver.has_any_chords() {
            if let Some(action) = resolver.resolve_single(active_contexts, event) {
                return KeybindingResult::Action(action);
            }
            return KeybindingResult::Unhandled;
        }

        match chord.process_key(event, &resolver, active_contexts) {
            ChordResult::Matched(action) => KeybindingResult::Action(action),
            ChordResult::PrefixMatch => KeybindingResult::PendingChord,
            ChordResult::Cancelled => KeybindingResult::ChordCancelled,
            ChordResult::NoMatch => {
                if let Some(action) = resolver.resolve_single(active_contexts, event) {
                    KeybindingResult::Action(action)
                } else {
                    KeybindingResult::Unhandled
                }
            }
        }
    }

    /// Whether a chord is currently pending.
    pub fn is_chord_pending(&self) -> bool {
        self.chord
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_pending()
    }

    /// Get display text for an action (the canonical key string).
    pub fn display_text_for_action(
        &self,
        action: &Action,
        active_contexts: &[KeybindingContext],
    ) -> Option<String> {
        let resolver = self
            .resolver
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        resolver.display_text_for_action(action, active_contexts)
    }

    /// Get the display text for the pending chord (e.g., "Ctrl+K ...").
    ///
    /// Returns `None` if no chord is pending.
    pub fn pending_chord_display(&self) -> Option<String> {
        let chord = self
            .chord
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if chord.is_pending() {
            Some(
                chord
                    .pending_keys()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        } else {
            None
        }
    }

    /// Check if the pending chord has timed out.
    ///
    /// If a chord timed out, attempts to resolve the pending prefix as
    /// a complete binding (fallback). For example, single Esc times out
    /// from the Esc-Esc chord and resolves to `ChatCancel`.
    ///
    /// The TUI should call this on its tick interval. When `Some(action)`
    /// is returned, the caller should execute it via `action_to_command`.
    pub fn check_chord_timeout(&self, active_contexts: &[KeybindingContext]) -> Option<Action> {
        let timed_out_keys = self
            .chord
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .check_timeout()?;

        // Try to resolve the timed-out prefix as a complete binding.
        let resolver = self
            .resolver
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        resolver.resolve_sequence(active_contexts, &timed_out_keys)
    }

    /// Get all bindings for a context (for help overlay).
    pub fn bindings_for_context(&self, context: KeybindingContext) -> Vec<(String, String)> {
        let resolver = self
            .resolver
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        resolver.bindings_for_context(context)
    }

    /// Spawn a background task that reloads bindings when the file changes.
    fn spawn_reload_task(
        mut rx: broadcast::Receiver<KeybindingsChanged>,
        resolver: Arc<RwLock<BindingResolver>>,
        config_dir: PathBuf,
    ) {
        tokio::spawn(async move {
            while let Ok(_event) = rx.recv().await {
                info!("keybindings file changed, reloading");
                let defaults = default_bindings();
                let (user_bindings, warnings) = load_user_bindings(&config_dir);
                for w in &warnings {
                    warn!("keybinding reload: {w}");
                }
                let merged = merge_bindings(defaults, user_bindings);
                match resolver.write() {
                    Ok(mut r) => r.replace(merged),
                    Err(e) => e.into_inner().replace(merged),
                }
                info!("keybindings reloaded");
            }
        });
    }
}

#[cfg(test)]
#[path = "manager.test.rs"]
mod tests;
