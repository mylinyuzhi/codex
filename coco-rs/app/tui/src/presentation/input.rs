//! Presentation view models for composer-adjacent input surfaces.

use crate::state::AppState;
use crate::widgets::suggestion_popup::SuggestionItem;

#[derive(Debug, Clone)]
pub(crate) struct InlinePopupView {
    pub(crate) items: Vec<SuggestionItem>,
    pub(crate) selected: usize,
}

impl InlinePopupView {
    pub(crate) fn item_count(&self) -> usize {
        self.items.len()
    }
}

pub(crate) fn inline_popup_view(state: &AppState) -> Option<InlinePopupView> {
    if state.ui.interaction.active_prompt.is_some() {
        return None;
    }
    let popup = state.ui.interaction.popup.as_ref()?;
    let suggestions = state.ui.active_suggestions.as_ref()?;
    if popup.kind() != suggestions.kind || suggestions.items.is_empty() {
        return None;
    }
    Some(InlinePopupView {
        items: suggestions.items.clone(),
        selected: suggestions.selected,
    })
}

#[cfg(test)]
#[path = "input.test.rs"]
mod tests;
