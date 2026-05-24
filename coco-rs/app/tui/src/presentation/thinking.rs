//! Shared rendering helpers for assistant thinking blocks.

use std::time::Duration;

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::constants;
use crate::i18n::t;
use crate::presentation::styles::UiStyles;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThinkingDisplay {
    Collapsed,
    Expanded {
        max_body_lines: usize,
        truncated_hint: &'static str,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ThinkingRenderInput<'a> {
    pub(crate) content: &'a str,
    pub(crate) duration_ms: Option<i64>,
    pub(crate) reasoning_tokens: Option<i64>,
    pub(crate) display: ThinkingDisplay,
}

pub(crate) fn render_thinking_block(
    input: ThinkingRenderInput<'_>,
    styles: UiStyles<'_>,
) -> Vec<Line<'static>> {
    let mut parts = vec![t!("chat.thinking_label").to_string()];
    if let Some(ms) = input.duration_ms.filter(|ms| *ms >= 0) {
        let seconds = format!("{:.1}", ms as f64 / 1_000.0);
        parts.push(t!("chat.thinking_duration", seconds = seconds).to_string());
    }
    if let Some(tokens) = input.reasoning_tokens.filter(|tokens| *tokens > 0) {
        parts.push(
            t!(
                "chat.thinking_reasoning_tokens",
                count = format_reasoning_tokens(tokens)
            )
            .to_string(),
        );
    }
    let mut lines = vec![Line::from(
        Span::raw(format!("⏺ {}", parts.join(" · ")))
            .fg(styles.thinking())
            .dim()
            .italic(),
    )];

    let ThinkingDisplay::Expanded {
        max_body_lines,
        truncated_hint,
    } = input.display
    else {
        return lines;
    };

    if input.content.is_empty() {
        return lines;
    }

    let mut iter = input.content.lines();
    for line in iter.by_ref().take(max_body_lines) {
        lines.push(Line::from(
            Span::raw(format!("  {}", transcript_safe_line(line)))
                .fg(styles.thinking())
                .dim()
                .italic(),
        ));
    }
    if iter.next().is_some() {
        lines.push(Line::from(
            Span::raw(format!("  {truncated_hint}"))
                .fg(styles.thinking())
                .dim()
                .italic(),
        ));
    }
    lines
}

pub(crate) fn estimate_reasoning_tokens(thinking: &str) -> i64 {
    (thinking.split_whitespace().count() as f64 * constants::THINKING_TOKEN_MULTIPLIER) as i64
}

pub(crate) fn format_duration_seconds(duration: Duration) -> String {
    format!("{:.1}s", duration.as_secs_f64())
}

fn format_reasoning_tokens(tokens: i64) -> String {
    if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn transcript_safe_line(line: &str) -> String {
    const MAX_CHARS: usize = crate::presentation::transcript::TRANSCRIPT_LINE_CHAR_CAP;
    if line.chars().count() <= MAX_CHARS {
        return line.to_string();
    }
    let mut out = line
        .chars()
        .take(MAX_CHARS.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

#[cfg(test)]
#[path = "thinking.test.rs"]
mod tests;
