//! Autocomplete systems for input suggestions.
//!
//! Four parallel autocomplete systems, each with:
//! - Trigger character detection (@, /, @agent-, @#)
//! - Debounced async search (100ms)
//! - Cancel-on-change (previous search aborted)
//! - Results delivered via mpsc channel
//!
//! TS: src/hooks/useTypeahead.ts, file_search.rs, skill_search.rs, agent_search.rs, symbol_search.rs

pub mod agent_search;
pub mod file_search;
pub mod skill_search;
pub mod symbol_search;

pub use agent_search::AgentInfo;
pub use agent_search::AgentSearchManager;
pub use file_search::FileSearchEvent;
pub use file_search::FileSearchManager;
pub use skill_search::SkillInfo;
pub use skill_search::SkillSearchEvent;
pub use skill_search::SkillSearchManager;
pub use symbol_search::SymbolSearchEvent;
pub use symbol_search::SymbolSearchManager;

use crate::widgets::suggestion_popup::SuggestionItem;

/// Shared suggestion state for any autocomplete type.
#[derive(Debug, Clone)]
pub struct SuggestionState {
    /// The query that produced these suggestions.
    pub query: String,
    /// Start position in input where the trigger was detected.
    pub start_pos: i32,
    /// Matching suggestions.
    pub items: Vec<SuggestionItem>,
    /// Currently selected index.
    pub selected: i32,
}

impl SuggestionState {
    /// Create a new suggestion state.
    pub fn new(query: String, start_pos: i32, items: Vec<SuggestionItem>) -> Self {
        Self {
            query,
            start_pos,
            items,
            selected: 0,
        }
    }

    /// Move selection up.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn select_next(&mut self) {
        if self.selected < self.items.len() as i32 - 1 {
            self.selected += 1;
        }
    }

    /// Get the currently selected item.
    pub fn selected_item(&self) -> Option<&SuggestionItem> {
        self.items.get(self.selected as usize)
    }

    /// Whether there are any suggestions.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}
