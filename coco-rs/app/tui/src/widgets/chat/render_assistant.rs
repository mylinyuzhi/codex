//! Assistant-side message renderers — plain text (markdown), thinking
//! block (collapsible with token estimate), redacted thinking, tool-use
//! call, advisor message.

use std::time::Duration;

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::constants;
use crate::i18n::t;
use crate::state::session::MessageContent;
use crate::state::session::ToolUseStatus;

/// Format a `Duration` for the tool-use elapsed badge.
///
/// - < 1s: milliseconds (`250ms`)
/// - < 60s: seconds with one decimal (`12.3s`)
/// - >= 60s: minutes + whole seconds (`3m 4s`)
fn format_elapsed(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        let secs = d.as_secs();
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

pub(super) fn try_render<'a>(
    w: &ChatWidget<'a>,
    content: &'a MessageContent,
    lines: &mut Vec<Line<'a>>,
) -> Option<()> {
    match content {
        MessageContent::AssistantText(text) => {
            let md_lines = crate::widgets::markdown::markdown_to_lines(text, w.theme, w.width);
            lines.extend(md_lines);
            Some(())
        }
        MessageContent::Thinking {
            content,
            duration_ms,
        } => {
            if w.show_thinking {
                let token_est = (content.split_whitespace().count() as f64
                    * constants::THINKING_TOKEN_MULTIPLIER) as i64;
                let dur = duration_ms
                    .map(|ms| format!(" ({ms}ms)"))
                    .unwrap_or_default();
                lines.push(Line::from(
                    Span::raw(
                        t!("chat.thinking_tokens_dur", count = token_est, dur = dur).to_string(),
                    )
                    .fg(w.theme.thinking)
                    .italic(),
                ));
                for line in content.lines().take(5) {
                    lines.push(Line::from(
                        Span::raw(format!("  │ {line}")).fg(w.theme.thinking).dim(),
                    ));
                }
                if content.lines().count() > 5 {
                    lines.push(Line::from(Span::raw("  │ ...").fg(w.theme.thinking).dim()));
                }
            } else {
                let token_est = (content.split_whitespace().count() as f64
                    * constants::THINKING_TOKEN_MULTIPLIER) as i64;
                lines.push(Line::from(
                    Span::raw(t!("chat.thinking_est", count = token_est).to_string())
                        .fg(w.theme.thinking)
                        .dim(),
                ));
            }
            Some(())
        }
        MessageContent::RedactedThinking => {
            lines.push(Line::from(
                Span::raw(t!("chat.redacted_thinking").to_string())
                    .fg(w.theme.thinking)
                    .dim(),
            ));
            Some(())
        }
        MessageContent::ToolUse {
            tool_name,
            call_id,
            input_preview,
            status,
        } => {
            let (icon, color) = match status {
                ToolUseStatus::Queued => ("◌", w.theme.text_dim),
                ToolUseStatus::Running => ("⏳", w.theme.tool_running),
                ToolUseStatus::Completed => ("✓", w.theme.tool_completed),
                ToolUseStatus::Failed => ("✗", w.theme.tool_error),
            };
            let preview = if input_preview.len() > constants::TOOL_DESCRIPTION_MAX_CHARS as usize {
                format!(
                    "{}...",
                    &input_preview[..constants::TOOL_DESCRIPTION_MAX_CHARS as usize - 3]
                )
            } else {
                input_preview.clone()
            };
            // Elapsed time badge: `(250ms)` / `(1.2s)` / `(3m 4s)` tail-aligned
            // after the preview. Sourced from the matching ToolExecution by
            // call_id so running tools tick forward via SpinnerTick redraws
            // and completed tools freeze at their final duration.
            let elapsed_badge = w
                .tool_executions
                .iter()
                .find(|t| t.call_id == *call_id)
                .map(|t| format!(" ({})", format_elapsed(t.elapsed())))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::raw(format!("  {icon} ")).fg(color),
                Span::raw(format!("{tool_name}: "))
                    .fg(w.theme.text_dim)
                    .bold(),
                Span::raw(preview).fg(w.theme.text),
                Span::raw(elapsed_badge).fg(w.theme.text_dim).dim(),
            ]));
            Some(())
        }
        MessageContent::Advisor {
            advisor_id,
            content,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  📋 ").fg(w.theme.accent),
                Span::raw(format!("[advisor:{advisor_id}] "))
                    .fg(w.theme.text_dim)
                    .bold(),
            ]));
            let md_lines = crate::widgets::markdown::markdown_to_lines(content, w.theme, w.width);
            lines.extend(md_lines);
            Some(())
        }
        _ => None,
    }
}
