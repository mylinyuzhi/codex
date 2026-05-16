use super::*;

use crate::state::ActiveSuggestions;
use crate::state::AppState;
use crate::state::CommandOption;
use crate::state::CommandPaletteOverlay;
use crate::state::Overlay;
use crate::state::SuggestionKind;
use crate::widgets::suggestion_popup::SuggestionItem;

fn item(label: &str) -> SuggestionItem {
    SuggestionItem {
        label: label.into(),
        description: None,
        metadata: None,
    }
}

#[test]
fn inline_popup_view_prefers_active_suggestions() {
    let mut state = AppState::default();
    state.ui.active_suggestions = Some(ActiveSuggestions {
        kind: SuggestionKind::SlashCommand,
        items: vec![item("/help")],
        selected: 2,
        query: String::new(),
        trigger_pos: 0,
    });
    state
        .ui
        .set_overlay(Overlay::CommandPalette(CommandPaletteOverlay {
            commands: vec![CommandOption {
                name: "model".into(),
                description: None,
            }],
            filter: String::new(),
            selected: 0,
        }));

    let view = inline_popup_view(&state).expect("active suggestions should render");

    assert_eq!(view.selected, 2);
    assert_eq!(view.items.len(), 1);
    assert_eq!(view.items[0].label, "/help");
}

#[test]
fn inline_popup_view_filters_command_palette_items() {
    let mut state = AppState::default();
    state
        .ui
        .set_overlay(Overlay::CommandPalette(CommandPaletteOverlay {
            commands: vec![
                CommandOption {
                    name: "model".into(),
                    description: Some("Set model".into()),
                },
                CommandOption {
                    name: "clear".into(),
                    description: Some("Clear chat".into()),
                },
            ],
            filter: "cle".into(),
            selected: -3,
        }));

    let view = inline_popup_view(&state).expect("matching command should render");

    assert_eq!(view.selected, 0);
    assert_eq!(view.items.len(), 1);
    assert_eq!(view.items[0].label, "/clear");
    assert_eq!(view.items[0].description.as_deref(), Some("Clear chat"));
}

#[test]
fn inline_popup_view_returns_none_when_no_rows_match() {
    let mut state = AppState::default();
    state
        .ui
        .set_overlay(Overlay::CommandPalette(CommandPaletteOverlay {
            commands: vec![CommandOption {
                name: "model".into(),
                description: None,
            }],
            filter: "zzz".into(),
            selected: 0,
        }));

    assert!(inline_popup_view(&state).is_none());
}
