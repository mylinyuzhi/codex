use pretty_assertions::assert_eq;

use super::*;
use crate::state::session::MessageContent;
use crate::theme::Theme;

#[test]
fn finalized_history_lines_render_committed_assistant_message() {
    let theme = Theme::default();
    let messages = vec![ChatMessage::assistant_text("a1", "hello")];

    let lines = render_finalized_history_lines(
        &messages,
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
    let messages = vec![ChatMessage::user_text("u1", "hello")];

    let lines = render_finalized_history_lines(
        &messages,
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
    let mut meta = ChatMessage::system_text("m1", "system reminder");
    meta.is_meta = true;

    let lines = render_finalized_history_lines(
        &[meta],
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
    let messages = vec![ChatMessage {
        id: "t1".into(),
        role: crate::state::ChatRole::Assistant,
        content: MessageContent::Thinking {
            content: "Need to inspect files.".into(),
            duration_ms: Some(1300),
            reasoning_tokens: Some(15),
        },
        is_meta: false,
        created_at_ms: crate::state::session::now_ms(),
        is_compact_summary: false,
        is_visible_in_transcript_only: false,
        permission_mode: None,
    }];

    let lines = render_finalized_history_lines(
        &messages,
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
    let messages = vec![ChatMessage::assistant_text("a1", "hello")];

    let replay = render_replay_history_lines(&messages, options(&theme, 40), 4);

    assert_eq!(plain_lines(&replay.lines), vec!["⏺ hello", ""]);
    assert_eq!(replay.omitted_messages, 0);
}

#[test]
fn replay_history_lines_truncates_at_message_boundaries_with_marker() {
    let theme = Theme::default();
    let messages = vec![
        ChatMessage::assistant_text("a1", "one"),
        ChatMessage::assistant_text("a2", "two"),
        ChatMessage::assistant_text("a3", "three"),
    ];

    let replay = render_replay_history_lines(&messages, options(&theme, 40), 5);

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
