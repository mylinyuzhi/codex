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
            // Mirror the invocation row: the result block reuses the
            // `●` glyph so the eye groups call+result. Status is
            // colour-encoded (green ⇒ completed). Body is indented
            // four spaces past the gutter so column alignment matches
            // the tool-use header.
            lines.push(Line::from(vec![
                Span::raw("  ● ").fg(w.theme.tool_completed),
                Span::raw(tool_name.clone()).fg(w.theme.text).bold(),
            ]));
            let total = output.lines().count();
            for line in output.lines().take(15) {
                lines.push(Line::from(
                    Span::raw(format!("    {line}")).fg(w.theme.text),
                ));
            }
            if total > 15 {
                lines.push(Line::from(
                    Span::raw(format!("    … ({} more lines)", total - 15))
                        .fg(w.theme.text_dim)
                        .italic(),
                ));
            }
            Some(())
        }
        MessageContent::ToolError { tool_name, error } => {
            lines.push(Line::from(vec![
                Span::raw("  ● ").fg(w.theme.tool_error),
                Span::raw(tool_name.clone()).fg(w.theme.text).bold(),
                Span::raw(": ").fg(w.theme.text_dim),
                Span::raw(error.as_str()).fg(w.theme.error),
            ]));
            Some(())
        }
        MessageContent::ToolRejected { tool_name, reason } => {
            // ⊘ (circled division slash) reads as "blocked" without
            // implying error or success — TS uses similar gating
            // glyphs. Warning colour keeps the row visible but not
            // alarming.
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
