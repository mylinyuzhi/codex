//! Presentation view models for composer-adjacent input surfaces.

use crate::state::AppState;
use crate::state::Overlay;
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
    if let Some(suggestions) = state.ui.active_suggestions.as_ref()
        && !suggestions.items.is_empty()
    {
        return Some(InlinePopupView {
            items: suggestions.items.clone(),
            selected: suggestions.selected,
        });
    }

    let Overlay::CommandPalette(cp) = state.ui.active_overlay()? else {
        return None;
    };

    let filter_lower = cp.filter.to_lowercase();
    let items: Vec<SuggestionItem> = cp
        .commands
        .iter()
        .filter(|cmd| filter_lower.is_empty() || cmd.name.to_lowercase().contains(&filter_lower))
        .map(|cmd| SuggestionItem {
            label: format!("/{}", cmd.name),
            description: cmd.description.clone(),
            metadata: None,
        })
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(InlinePopupView {
            items,
            selected: cp.selected.max(0) as usize,
        })
    }
}

#[cfg(test)]
#[path = "input.test.rs"]
mod tests;
