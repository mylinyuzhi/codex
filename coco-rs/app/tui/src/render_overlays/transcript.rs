//! Transcript overlay renderer.
//!
//! Reuses the existing `ChatWidget` with `show_system_reminders=true`
//! (so meta messages are included). The overlay mod wraps the body in
//! the standard centered Paragraph; we produce the body as plain text
//! by walking the chat widget's line builder.
//!
//! A footer line carries the action hints, mirroring TS
//! `TranscriptModeFooter` (`screens/REPL.tsx:321-362`):
//!   `Showing detailed transcript · {toggleChord} to toggle · {showAllChord} to {collapse|show all}`
//! Both chords are looked up via `kb_handle.display_for(...)` so user
//! re-bindings show through.

use coco_keybindings::KeybindingAction;
use ratatui::prelude::Color;

use crate::i18n::t;
use crate::keybinding_bridge::KeybindingContext as TuiCtx;
use crate::state::AppState;
use crate::state::overlay::TranscriptOverlay;
use crate::theme::Theme;

pub(super) fn transcript_overlay_content(
    state: &AppState,
    overlay: &TranscriptOverlay,
    theme: &Theme,
) -> (String, String, Color) {
    let title = t!("transcript.title").to_string();

    // Reuse `ChatWidget`'s rendering pipeline so the transcript looks
    // like the live chat — `build_lines_owned()` honours the overlay's
    // `show_all` flag (mirrors TS `showAllInTranscript`).
    let mut chat = crate::widgets::ChatWidget::new(&state.session.messages, theme)
        .show_thinking(true)
        .show_system_reminders(overlay.show_all)
        .tool_executions(&state.session.tool_executions)
        .syntax_highlighting(state.ui.display_settings.syntax_highlighting)
        .kb_handle(&state.ui.kb_handle);
    if !state.ui.collapsed_tools.is_empty() {
        chat = chat.collapsed_tools(&state.ui.collapsed_tools);
    }

    // Convert the chat widget's lines to a single body string. Scroll
    // by skipping `overlay.scroll` lines from the top — the overlay
    // wrapper handles the trailing clip via Paragraph rendering.
    let lines = chat.build_lines_owned();
    let body_text = lines
        .iter()
        .skip(overlay.scroll.max(0) as usize)
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    let body_text = if body_text.is_empty() {
        t!("transcript.empty").to_string()
    } else {
        body_text
    };

    // Footer hint — dynamic chord lookup. TS `TranscriptModeFooter`
    // surfaces `app:toggleTranscript` (the same chord that opens the
    // overlay also closes it) for the exit hint, not `transcript:exit`
    // — so the user's mental model is one chord owns the toggle.
    let toggle_chord = state
        .ui
        .kb_handle
        .display_for(&KeybindingAction::AppToggleTranscript, TuiCtx::Chat)
        .unwrap_or_else(|| "ctrl+o".to_string());
    let show_all_chord = state
        .ui
        .kb_handle
        .display_for(
            &KeybindingAction::TranscriptToggleShowAll,
            TuiCtx::Scrollable,
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

    let body = format!("{body_text}\n\n{footer}");
    (title, body, theme.primary)
}
