//! Binding resolution.
//!
//! Resolves key events to actions using the merged binding table.
//! Uses last-match-wins semantics so user overrides shadow defaults.

use crossterm::event::KeyEvent;

use crate::action::Action;
use crate::context::KeybindingContext;
use crate::key::KeyCombo;
use crate::key::KeySequence;
use crate::key::key_event_to_combo;

/// A single keybinding entry.
#[derive(Debug, Clone)]
pub struct Binding {
    pub context: KeybindingContext,
    pub sequence: KeySequence,
    pub action: Action,
}

/// Resolves key events to actions against the binding table.
#[derive(Debug, Clone)]
pub struct BindingResolver {
    bindings: Vec<Binding>,
    has_chords: bool,
}

impl BindingResolver {
    pub fn new(bindings: Vec<Binding>) -> Self {
        let has_chords = bindings.iter().any(|b| b.sequence.is_chord());
        Self {
            bindings,
            has_chords,
        }
    }

    /// Replace the binding table (e.g., after hot-reload).
    pub fn replace(&mut self, bindings: Vec<Binding>) {
        self.has_chords = bindings.iter().any(|b| b.sequence.is_chord());
        self.bindings = bindings;
    }

    /// Resolve a single key event (non-chord) to an action.
    ///
    /// Checks the active contexts plus `Global` fallback.
    /// Uses last-match-wins: iterates all bindings and keeps the last match.
    pub fn resolve_single(
        &self,
        active_contexts: &[KeybindingContext],
        event: &KeyEvent,
    ) -> Option<Action> {
        let combo = key_event_to_combo(event)?;
        self.resolve_sequence(active_contexts, &[combo])
    }

    /// Resolve a sequence of key combos (chord or single) to an action.
    ///
    /// Uses last-match-wins semantics.
    pub fn resolve_sequence(
        &self,
        active_contexts: &[KeybindingContext],
        sequence: &[KeyCombo],
    ) -> Option<Action> {
        let mut result = None;
        for binding in &self.bindings {
            if !is_context_active(binding.context, active_contexts) {
                continue;
            }
            if binding.sequence.matches_exactly(sequence) {
                result = Some(binding.action.clone());
            }
        }
        result
    }

    /// Check if any binding has `sequence` as a prefix (for chord detection).
    pub fn has_prefix_match(
        &self,
        active_contexts: &[KeybindingContext],
        sequence: &[KeyCombo],
    ) -> bool {
        self.bindings.iter().any(|binding| {
            is_context_active(binding.context, active_contexts)
                && binding.sequence.is_prefix_of(sequence)
        })
    }

    /// Get the display text for an action (the canonical key string).
    ///
    /// Returns the key string from the last matching binding (user override
    /// wins over default). Iterates in reverse for efficiency.
    pub fn display_text_for_action(
        &self,
        action: &Action,
        active_contexts: &[KeybindingContext],
    ) -> Option<String> {
        for binding in self.bindings.iter().rev() {
            if &binding.action == action && is_context_active(binding.context, active_contexts) {
                return Some(binding.sequence.to_string());
            }
        }
        None
    }

    /// Get all bindings for a specific context (for help overlay).
    pub fn bindings_for_context(&self, context: KeybindingContext) -> Vec<(String, String)> {
        let mut result = Vec::new();
        for binding in &self.bindings {
            if binding.context == context {
                result.push((binding.sequence.to_string(), binding.action.to_string()));
            }
        }
        result
    }

    /// Whether any binding uses a chord (multi-key sequence).
    ///
    /// Cached at construction time — O(1).
    pub fn has_any_chords(&self) -> bool {
        self.has_chords
    }

    /// Get all bindings.
    pub fn all_bindings(&self) -> &[Binding] {
        &self.bindings
    }
}

/// Check if a binding's context is currently active.
///
/// `Global` is always active. Otherwise the binding's context must be in
/// the active set.
fn is_context_active(binding_ctx: KeybindingContext, active: &[KeybindingContext]) -> bool {
    binding_ctx == KeybindingContext::Global || active.contains(&binding_ctx)
}

#[cfg(test)]
#[path = "resolver.test.rs"]
mod tests;
