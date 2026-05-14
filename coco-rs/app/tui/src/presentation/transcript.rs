//! Transcript overlay presentation.

use coco_keybindings::KeybindingAction;
use ratatui::prelude::Color;

use crate::i18n::t;
use crate::keybinding_bridge::KeybindingContext as TuiContext;
use crate::state::AppState;
use crate::state::overlay::TranscriptOverlay;
use crate::theme::Theme;

pub(crate) fn transcript_overlay_content(
    state: &AppState,
    overlay: &TranscriptOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let title = t!("transcript.title").to_string();
    let mut chat = crate::widgets::ChatWidget::new(&state.session.messages, theme)
        .show_thinking(true)
        .show_system_reminders(overlay.show_all)
        .tool_executions(&state.session.tool_executions)
        .syntax_highlighting(state.ui.display_settings.syntax_highlighting)
        .kb_handle(&state.ui.kb_handle);
    if !state.ui.collapsed_tools.is_empty() {
        chat = chat.collapsed_tools(&state.ui.collapsed_tools);
    }

    let lines = chat.build_lines_owned();
    let body_text = lines
        .iter()
        .skip(overlay.scroll.max(0) as usize)
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let body_text = if body_text.is_empty() {
        t!("transcript.empty").to_string()
    } else {
        body_text
    };

    let toggle_chord = state
        .ui
        .kb_handle
        .display_for(&KeybindingAction::AppToggleTranscript, TuiContext::Chat)
        .unwrap_or_else(|| "ctrl+o".to_string());
    let show_all_chord = state
        .ui
        .kb_handle
        .display_for(
            &KeybindingAction::TranscriptToggleShowAll,
            TuiContext::Scrollable,
        )
        .unwrap_or_else(|| "ctrl+e".to_string());
    let show_all_label = if overlay.show_all {
        t!("transcript.hint_show_all_on").to_string()
    } else {
        t!("transcript.hint_show_all_off").to_string()
    };
    let footer = t!(
        "transcript.hint_footer",
        toggle = toggle_chord.as_str(),
        show_all_chord = show_all_chord.as_str(),
        show_all = show_all_label.as_str(),
    )
    .to_string();

    (title, format!("{body_text}\n\n{footer}"), theme.primary)
}

#[cfg(test)]
#[path = "transcript.test.rs"]
mod tests;
