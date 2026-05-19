use pretty_assertions::assert_eq;
use uuid::Uuid;

use super::*;
use crate::state::derive::test_helpers;
use crate::theme::Theme;

#[test]
fn finalized_history_lines_render_committed_assistant_message() {
    let theme = Theme::default();
    let cells = vec![test_helpers::assistant_text_cell("hello")];

    let lines = render_finalized_history_lines(
        &cells,
        HistoryLineRenderOptions {
            styles: UiStyles::new(&theme),
            width: 40,
            syntax_highlighting: SyntaxHighlighting::Disabled,
            show_system_reminders: false,
            show_thinking: false,
            kb_handle: None,
        },
    );

    assert_eq!(plain_lines(&lines), vec!["⏺ hello", ""]);
}

#[test]
fn finalized_history_lines_do_not_emit_active_busy_tail() {
    let theme = Theme::default();
    let cells = vec![test_helpers::user_text_cell(Uuid::new_v4(), "hello")];

    let lines = render_finalized_history_lines(
        &cells,
        HistoryLineRenderOptions {
            styles: UiStyles::new(&theme),
            width: 40,
            syntax_highlighting: SyntaxHighlighting::Disabled,
            show_system_reminders: false,
            show_thinking: false,
            kb_handle: None,
        },
    );

    assert_eq!(plain_lines(&lines), vec!["❯ hello", ""]);
}

#[test]
fn finalized_history_lines_collapse_meta_by_default() {
    let theme = Theme::default();
    let cells = vec![test_helpers::info_cell("", "system reminder")];

    let lines = render_finalized_history_lines(
        &cells,
        HistoryLineRenderOptions {
            styles: UiStyles::new(&theme),
            width: 40,
            syntax_highlighting: SyntaxHighlighting::Disabled,
            show_system_reminders: false,
            show_thinking: false,
            kb_handle: None,
        },
    );

    assert_eq!(plain_lines(&lines), vec!["  # [system] system reminder"]);
}

#[test]
fn finalized_history_lines_show_collapsed_thinking_without_per_item_toggle_hint() {
    let theme = Theme::default();
    let kb_handle = crate::keybinding_resolver::KeybindingHandle::from_defaults();
    // Phase 3d (§5): reasoning metadata now rides on
    // `CellKind::AssistantThinking { duration_ms, reasoning_tokens }`.
    // `TranscriptView::record_reasoning_tokens` (called from
    // `on_turn_completed`) stamps the values onto the latest thinking
    // cell so the header renders the full `Thinking · 1.3s · 15
    // reasoning tokens` line. Tests bypass the engine flow and use the
    // `_with_metadata` helper directly.
    let cells = vec![test_helpers::assistant_thinking_cell_with_metadata(
        "Need to inspect files.",
        1300,
        15,
    )];

    let lines = render_finalized_history_lines(
        &cells,
        HistoryLineRenderOptions {
            styles: UiStyles::new(&theme),
            width: 80,
            syntax_highlighting: SyntaxHighlighting::Disabled,
            show_system_reminders: false,
            show_thinking: false,
            kb_handle: Some(&kb_handle),
        },
    );

    assert_eq!(
        plain_lines(&lines),
        vec!["⏺ Thinking · 1.3s · 15 reasoning tokens", "",]
    );
}

#[test]
fn replay_history_lines_keeps_all_rows_under_cap() {
    let theme = Theme::default();
    let cells = vec![test_helpers::assistant_text_cell("hello")];

    let replay = render_replay_history_lines(&cells, options(&theme, 40), 4);

    assert_eq!(plain_lines(&replay.lines), vec!["⏺ hello", ""]);
    assert_eq!(replay.omitted_messages, 0);
}

#[test]
fn replay_history_lines_truncates_at_message_boundaries_with_marker() {
    let theme = Theme::default();
    let cells = vec![
        test_helpers::assistant_text_cell("one"),
        test_helpers::assistant_text_cell("two"),
        test_helpers::assistant_text_cell("three"),
    ];

    let replay = render_replay_history_lines(&cells, options(&theme, 40), 5);

    assert_eq!(replay.omitted_messages, 2);
    assert_eq!(
        plain_lines(&replay.lines),
        vec![
            "... 2 older messages retained in transcript, not replayed",
            "    open transcript pager for full history",
            "",
            "⏺ three",
            "",
        ]
    );
}

fn options(theme: &Theme, width: u16) -> HistoryLineRenderOptions<'_> {
    HistoryLineRenderOptions {
        styles: UiStyles::new(theme),
        width,
        syntax_highlighting: SyntaxHighlighting::Disabled,
        show_system_reminders: false,
        show_thinking: false,
        kb_handle: None,
    }
}

fn plain_lines(lines: &[Line<'_>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}
