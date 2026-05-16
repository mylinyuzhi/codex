//! Finalized transcript rendering for native history emission.
// S2 adapter: this initially reuses the existing chat renderer in committed-only
// mode while the native history cell renderer is carved out.
#![allow(dead_code)]

use ratatui::text::Line;

use crate::display_settings::SyntaxHighlighting;
use crate::presentation::styles::UiStyles;
use crate::state::session::ChatMessage;
use crate::widgets::ChatWidget;

pub(crate) const DEFAULT_MAX_REFLOW_ROWS: usize = 9_000;

#[derive(Debug, Clone, Copy)]
pub(crate) struct HistoryLineRenderOptions<'a> {
    pub(crate) styles: UiStyles<'a>,
    pub(crate) width: u16,
    pub(crate) syntax_highlighting: SyntaxHighlighting,
    pub(crate) show_system_reminders: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HistoryReplayLines {
    pub(crate) lines: Vec<Line<'static>>,
    pub(crate) omitted_messages: usize,
}

pub(crate) fn render_finalized_history_lines(
    messages: &[ChatMessage],
    options: HistoryLineRenderOptions<'_>,
) -> Vec<Line<'static>> {
    ChatWidget::new(messages, options.styles)
        .show_system_reminders(options.show_system_reminders)
        .width(options.width)
        .syntax_highlighting(options.syntax_highlighting)
        .build_lines_owned()
}

pub(crate) fn render_replay_history_lines(
    messages: &[ChatMessage],
    options: HistoryLineRenderOptions<'_>,
    max_rows: usize,
) -> HistoryReplayLines {
    let all_lines = render_finalized_history_lines(messages, options);
    if all_lines.len() <= max_rows || messages.is_empty() {
        return HistoryReplayLines {
            lines: all_lines,
            omitted_messages: 0,
        };
    }

    for start in 1..messages.len() {
        let omitted_messages = start;
        let mut lines = replay_truncation_marker(omitted_messages);
        lines.extend(render_finalized_history_lines(&messages[start..], options));
        if lines.len() <= max_rows {
            return HistoryReplayLines {
                lines,
                omitted_messages,
            };
        }
    }

    HistoryReplayLines {
        lines: replay_truncation_marker(messages.len()),
        omitted_messages: messages.len(),
    }
}

fn replay_truncation_marker(omitted_messages: usize) -> Vec<Line<'static>> {
    vec![
        Line::from(format!(
            "... {omitted_messages} older messages retained in transcript, not replayed"
        )),
        Line::from("    open transcript pager for full history"),
        Line::default(),
    ]
}

#[cfg(test)]
#[path = "history_lines.test.rs"]
mod tests;
