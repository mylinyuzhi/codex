//! Keybinding resolver.
//!
//! TS: keybindings/ resolver — given a context stack and a chord, return
//! the matching action (if any). Supports chord progressions: the first
//! combo may be a "pending" state that waits for the next combo before
//! committing to an action.

use std::collections::HashMap;

use crate::Keybinding;
use crate::parser;
use crate::parser::KeyChord;
use crate::parser::KeyCombo;

/// Result of feeding a combo into the resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveOutcome {
    /// No matching chord for this combo. The pending chord state resets.
    NoMatch,
    /// A chord completed and an action fires.
    Fire(String),
    /// The combo matched the prefix of a multi-combo chord; hold for the
    /// next combo to decide. Callers typically update the status bar
    /// with `"ctrl+k ..."` to signal the pending state.
    Pending,
}

/// Compiled keybinding map: (context, chord) → action. Callers feed
/// key combos through `feed()` which tracks chord state.
pub struct ChordResolver {
    /// Per-context binding map. `None` context means "global".
    by_context: HashMap<Option<String>, Vec<(KeyChord, String)>>,
    /// Accumulator for in-flight chords. Empty when no pending combo.
    pending: Vec<KeyCombo>,
}

impl ChordResolver {
    /// Build a resolver from a flat list of keybindings. Invalid
    /// entries (parse failures) are skipped silently — use `validator`
    /// to surface them.
    pub fn new(bindings: &[Keybinding]) -> Self {
        let mut by_context: HashMap<Option<String>, Vec<(KeyChord, String)>> = HashMap::new();
        for b in bindings {
            if let Ok(chord) = parser::parse_chord(&b.key) {
                by_context
                    .entry(b.context.clone())
                    .or_default()
                    .push((chord, b.action.clone()));
            }
        }
        Self {
            by_context,
            pending: Vec::new(),
        }
    }

    /// Feed a combo into the resolver. `context_stack` is the list of
    /// active contexts in precedence order (most-specific first). A
    /// binding in a more-specific context wins over the same chord in
    /// a broader context.
    pub fn feed(&mut self, combo: &KeyCombo, context_stack: &[&str]) -> ResolveOutcome {
        self.pending.push(combo.clone());
        let attempt = self.pending.clone();

        // Search from most-specific context outward. Global (None) is
        // always last.
        let mut contexts: Vec<Option<String>> = context_stack
            .iter()
            .map(|c| Some((*c).to_string()))
            .collect();
        contexts.push(None);

        let mut prefix_match = false;
        for ctx in &contexts {
            if let Some(bindings) = self.by_context.get(ctx) {
                for (chord, action) in bindings {
                    if combo_slice_equals(&chord.0, &attempt) {
                        // Full match — commit and reset state.
                        self.pending.clear();
                        return ResolveOutcome::Fire(action.clone());
                    }
                    if combo_slice_is_prefix(&chord.0, &attempt) {
                        prefix_match = true;
                    }
                }
            }
        }

        if prefix_match {
            ResolveOutcome::Pending
        } else {
            self.pending.clear();
            ResolveOutcome::NoMatch
        }
    }

    /// Reset any pending chord state. Call when the user hits Esc,
    /// focus changes, etc.
    pub fn reset(&mut self) {
        self.pending.clear();
    }

    /// Whether a partial chord is in flight.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

fn combo_slice_equals(chord: &[KeyCombo], attempt: &[KeyCombo]) -> bool {
    chord == attempt
}

fn combo_slice_is_prefix(chord: &[KeyCombo], attempt: &[KeyCombo]) -> bool {
    chord.len() > attempt.len() && chord[..attempt.len()] == attempt[..]
}

#[cfg(test)]
#[path = "resolver.test.rs"]
mod tests;
