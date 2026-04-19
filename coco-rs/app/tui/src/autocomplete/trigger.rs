//! Autocomplete trigger detection and suggestion-state refresh.
//!
//! TS: `src/hooks/useTypeahead.ts` — the hook that watches the input buffer
//! for trigger patterns (`/`, `@path`, `@agent-name`, `@#symbol`) and
//! populates the suggestion popup.
//!
//! The Rust port splits the same responsibilities:
//! - [`detect`] — pure function, `(text, cursor)` → `Option<Trigger>`.
//! - [`refresh_suggestions`] — mutates `UiState::active_suggestions` based
//!   on the current input, pulling synchronous items from session state
//!   (available commands for slash, agent list for agents). Async sources
//!   (file search, LSP symbols) are left as TODO — the popup appears with
//!   an empty item list until the search manager delivers results.

use super::agent_search::AgentSearchManager;
use crate::state::ActiveSuggestions;
use crate::state::AppState;
use crate::state::SuggestionKind;
use crate::widgets::suggestion_popup::SuggestionItem;

/// A detected trigger in the input buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub kind: SuggestionKind,
    /// Character offset of the trigger start (the `/` or `@`).
    pub pos: i32,
    /// Text the user typed after the trigger (filter query).
    pub query: String,
}

/// Scan `text` up to `cursor` for an active autocomplete trigger.
///
/// Rules (match TS):
/// - Leading `/` at position 0 with no space yet → `SlashCommand`. The
///   query is everything after the `/` up to cursor.
/// - `@#word` → `Symbol`.
/// - `@agent-word` → `Agent`.
/// - `@word` (anything else after `@`) → `File`.
///
/// `@`-mentions must be at text start or immediately after whitespace so
/// emails like `a@b.com` or tags already-accepted don't re-trigger.
pub fn detect(text: &str, cursor: i32) -> Option<Trigger> {
    let cursor = cursor.max(0) as usize;
    let chars: Vec<char> = text.chars().collect();
    let slice = &chars[..cursor.min(chars.len())];

    // Slash command: `/` at text start, no whitespace before cursor.
    if slice.first() == Some(&'/') && !slice[1..].iter().any(|c| c.is_whitespace()) {
        let query: String = slice[1..].iter().collect();
        return Some(Trigger {
            kind: SuggestionKind::SlashCommand,
            pos: 0,
            query,
        });
    }

    // Walk back from cursor to find the nearest unmarked `@`.
    let mut i = slice.len();
    while i > 0 {
        i -= 1;
        let c = slice[i];
        if c.is_whitespace() {
            return None;
        }
        if c == '@' {
            // `@` must be at text start or follow whitespace.
            if i > 0 && !slice[i - 1].is_whitespace() {
                return None;
            }
            let tail: String = slice[i + 1..].iter().collect();
            let (kind, query) = classify_at_trigger(&tail);
            return Some(Trigger {
                kind,
                pos: i as i32,
                query,
            });
        }
    }

    None
}

fn classify_at_trigger(tail: &str) -> (SuggestionKind, String) {
    if let Some(rest) = tail.strip_prefix('#') {
        return (SuggestionKind::Symbol, rest.to_string());
    }
    if let Some(rest) = tail.strip_prefix("agent-") {
        return (SuggestionKind::Agent, rest.to_string());
    }
    (SuggestionKind::File, tail.to_string())
}

/// Recompute `ui.active_suggestions` from the current input buffer.
///
/// Called after any input mutation (InsertChar, DeleteBackward, Yank, etc.).
/// Dismisses suggestions when no trigger is detected; installs or refreshes
/// them when one is. Synchronous sources (slash commands, agents) populate
/// items inline; async sources leave `items` empty pending a search result.
pub fn refresh_suggestions(state: &mut AppState) {
    let text = state.ui.input.text.clone();
    let cursor = state.ui.input.cursor;
    let Some(trigger) = detect(&text, cursor) else {
        state.ui.active_suggestions = None;
        return;
    };

    // Synchronous sources (Slash, Agent) populate items inline. Async
    // sources (File, Symbol) install the trigger with an empty item list so
    // the App event loop can see the query and dispatch to the matching
    // SearchManager — results arrive through the SearchResult event path.
    //
    // The Autocomplete keybinding context only activates when items is
    // non-empty, so arrow keys keep passing through to input editing until
    // results materialize.
    let items = match trigger.kind {
        SuggestionKind::SlashCommand => slash_items(state, &trigger.query),
        SuggestionKind::Agent => agent_items(state, &trigger.query),
        SuggestionKind::File | SuggestionKind::Symbol => Vec::new(),
    };

    // Preserve selected index across refreshes where possible — clamp to
    // the new item range so navigation stays stable as the user types.
    let prior_selected = state
        .ui
        .active_suggestions
        .as_ref()
        .filter(|s| s.kind == trigger.kind)
        .map(|s| s.selected)
        .unwrap_or(0);
    let selected = prior_selected.clamp(0, (items.len() as i32 - 1).max(0));

    state.ui.active_suggestions = Some(ActiveSuggestions {
        kind: trigger.kind,
        items,
        selected,
        query: trigger.query,
        trigger_pos: trigger.pos,
    });
}

fn agent_items(state: &AppState, query: &str) -> Vec<SuggestionItem> {
    // AgentSearchManager is a thin synchronous filter today; wrapping it
    // here keeps the trigger module agnostic of whether the data source
    // becomes async in the future.
    let manager = AgentSearchManager::new(state.session.available_agents.clone());
    manager.search(query)
}

/// Apply an async search result (from FileSearchManager or
/// SymbolSearchManager) to the active suggestion popup.
///
/// Returns `true` when the result was adopted. Drops stale results
/// silently when the user has already moved to a different trigger kind,
/// query, or dismissed the popup altogether — the manager's debounced
/// cancel isn't instant, so this guards against a slow result clobbering
/// a newer query.
pub fn apply_async_result(
    state: &mut AppState,
    kind: SuggestionKind,
    query: &str,
    suggestions: Vec<SuggestionItem>,
) -> bool {
    let Some(sug) = state.ui.active_suggestions.as_mut() else {
        return false;
    };
    if sug.kind != kind || sug.query != query {
        return false;
    }
    sug.items = suggestions;
    sug.selected = sug.selected.clamp(0, (sug.items.len() as i32 - 1).max(0));
    true
}

fn slash_items(state: &AppState, query: &str) -> Vec<SuggestionItem> {
    let q = query.to_lowercase();
    state
        .session
        .available_commands
        .iter()
        .filter(|(name, _)| q.is_empty() || name.to_lowercase().contains(&q))
        .map(|(name, desc)| SuggestionItem {
            label: format!("/{name}"),
            description: desc.clone(),
        })
        .collect()
}

#[cfg(test)]
#[path = "trigger.test.rs"]
mod tests;
