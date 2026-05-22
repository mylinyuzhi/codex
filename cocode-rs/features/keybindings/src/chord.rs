//! Chord state machine.
//!
//! Tracks multi-key sequences with a configurable timeout. When the first
//! key of a potential chord is pressed, the matcher enters `Pending` state
//! and waits for subsequent keys within the timeout window.

use std::time::Duration;

use crossterm::event::KeyEvent;
use std::time::Instant;

use crate::action::Action;
use crate::key::KeyCombo;
use crate::key::key_event_to_combo;
use crate::resolver::BindingResolver;

/// Default chord timeout: 1000ms between keystrokes.
const DEFAULT_CHORD_TIMEOUT: Duration = Duration::from_millis(1000);

/// Result of processing a key event through the chord matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordResult {
    /// A complete binding matched — execute this action.
    Matched(Action),
    /// The input so far is a prefix of one or more chord bindings.
    /// The UI should show a chord-pending indicator.
    PrefixMatch,
    /// No binding matches. The key should be handled normally.
    NoMatch,
    /// Escape was pressed while a chord was pending — cancel the chord.
    Cancelled,
}

/// Chord matching state machine.
pub struct ChordMatcher {
    /// Accumulated key combos during a pending chord.
    pending: Vec<KeyCombo>,
    /// When the first key of the current chord was pressed.
    started_at: Option<Instant>,
    /// Maximum time between chord keystrokes.
    timeout: Duration,
}

impl ChordMatcher {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            started_at: None,
            timeout: DEFAULT_CHORD_TIMEOUT,
        }
    }

    /// Whether a chord is currently in progress.
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The accumulated pending key sequence (for display).
    pub fn pending_keys(&self) -> &[KeyCombo] {
        &self.pending
    }

    /// Reset the chord state (e.g., on timeout or cancel).
    pub fn reset(&mut self) {
        self.pending.clear();
        self.started_at = None;
    }

    /// Process a key event and return the chord result.
    ///
    /// The resolver is used to check for prefix and exact matches against
    /// the current binding table.
    pub fn process_key(
        &mut self,
        event: &KeyEvent,
        resolver: &BindingResolver,
        active_contexts: &[crate::context::KeybindingContext],
    ) -> ChordResult {
        let now = Instant::now();

        if self.is_pending()
            && let Some(started) = self.started_at
            && now.duration_since(started) > self.timeout
        {
            self.reset();
        }

        // NOTE: Escape does NOT unconditionally cancel pending chords.
        // Instead, Esc goes through normal chord resolution so that
        // Esc-Esc chords can fire. If Esc doesn't match any chord
        // continuation, the "no match + pending" path below will cancel.

        let Some(combo) = key_event_to_combo(event) else {
            if self.is_pending() {
                self.reset();
                return ChordResult::Cancelled;
            }
            return ChordResult::NoMatch;
        };

        let mut candidate = self.pending.clone();
        candidate.push(combo);

        // Check for prefix match FIRST — if the candidate could be the
        // start of a longer chord, wait for more keys even if there is also
        // a single-key exact match (e.g., Esc matches ChatCancel but is
        // also a prefix of the Esc-Esc chord).
        if resolver.has_prefix_match(active_contexts, &candidate) {
            self.pending = candidate;
            if self.started_at.is_none() {
                self.started_at = Some(now);
            }
            return ChordResult::PrefixMatch;
        }

        if let Some(action) = resolver.resolve_sequence(active_contexts, &candidate) {
            self.reset();
            return ChordResult::Matched(action);
        }

        if self.is_pending() {
            self.reset();
            return ChordResult::Cancelled;
        }

        ChordResult::NoMatch
    }

    /// Check if the chord has timed out and return the timed-out keys.
    ///
    /// Returns the pending key sequence if it timed out, so the caller
    /// can attempt to resolve it as a complete (shorter) binding.
    /// For example, if Esc was pending (prefix of Esc-Esc chord) and
    /// timed out, the caller gets `[Esc]` back and can resolve it to
    /// `ChatCancel`.
    pub fn check_timeout(&mut self) -> Option<Vec<KeyCombo>> {
        if self.is_pending()
            && let Some(started) = self.started_at
            && Instant::now().duration_since(started) > self.timeout
        {
            let timed_out = std::mem::take(&mut self.pending);
            self.started_at = None;
            return Some(timed_out);
        }
        None
    }
}

impl Default for ChordMatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "chord.test.rs"]
mod tests;
