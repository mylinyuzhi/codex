//! Tool-result renderers — success, error, rejected, canceled, file-edit
//! diff, file-write result.

use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::i18n::t;
use crate::state::session::MessageContent;

pub(super) fn try_render<'a>(
    w: &ChatWidget<'a>,
    content: &'a MessageContent,
    lines: &mut Vec<Line<'a>>,
) -> Option<()> {
    match content {
        MessageContent::ToolSuccess { tool_name, output } => {
            lines.push(Line::from(vec![
                Span::raw("  ✓ ").fg(w.theme.tool_completed),
                Span::raw(format!("{tool_name}: ")).fg(w.theme.text_dim),
            ]));
            for line in output.lines().take(15) {
                lines.push(Line::from(
                    Span::raw(format!("    {line}")).fg(w.theme.text),
                ));
            }
            if output.lines().count() > 15 {
                lines.push(Line::from(
                    Span::raw(format!(
                        "    ... ({} more lines)",
                        output.lines().count() - 15
                    ))
                    .fg(w.theme.text_dim),
                ));
            }
            Some(())
        }
        MessageContent::ToolError { tool_name, error } => {
            lines.push(Line::from(vec![
                Span::raw("  ✗ ").fg(w.theme.tool_error),
                Span::raw(format!("{tool_name}: ")).fg(w.theme.text_dim),
                Span::raw(error.as_str()).fg(w.theme.error),
            ]));
            Some(())
        }
        MessageContent::ToolRejected { tool_name, reason } => {
            lines.push(Line::from(vec![
                Span::raw("  ⊘ ").fg(w.theme.warning),
                Span::raw(t!("chat.tool_rejected", tool_name = tool_name).to_string())
                    .fg(w.theme.text_dim),
                Span::raw(reason.as_str()).fg(w.theme.warning),
            ]));
            Some(())
        }
        MessageContent::ToolCanceled { tool_name } => {
            lines.push(Line::from(vec![
                Span::raw("  ⊘ ").fg(w.theme.text_dim),
                Span::raw(t!("chat.tool_canceled", tool_name = tool_name).to_string())
                    .fg(w.theme.text_dim)
                    .italic(),
            ]));
            Some(())
        }
        MessageContent::FileEditDiff { path, diff, .. } => {
            lines.push(Line::from(vec![
                Span::raw("  📝 ").fg(w.theme.accent),
                Span::raw(path.as_str()).fg(w.theme.primary).underlined(),
            ]));
            let diff_lines =
                crate::widgets::diff_display::render_diff_lines(diff, w.theme, w.width);
            lines.extend(diff_lines);
            Some(())
        }
        MessageContent::FileWriteResult {
            path,
            bytes_written,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  ✓ ").fg(w.theme.tool_completed),
                Span::raw(t!("chat.wrote_bytes").to_string()).fg(w.theme.text_dim),
                Span::raw(path.as_str()).fg(w.theme.primary),
                Span::raw(format!(" ({bytes_written} bytes)")).fg(w.theme.text_dim),
            ]));
            Some(())
        }
        _ => None,
    }
}
