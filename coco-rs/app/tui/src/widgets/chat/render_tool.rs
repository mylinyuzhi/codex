//! Tool-result cell renderer. Reads `tool_name`, `output`, and
//! `is_error` from `cell.source: Arc<Message::ToolResult>`.
//!
//! Phase 3d (§6): dispatches on `cell.kind` / `cell.source`.
//! `MessageContent::FileEditDiff`, `FileWriteResult`, `ToolRejected`,
//! `ToolCanceled` were TUI-only variants that the engine flow never
//! emits — their match arms went away with `MessageContent`.

use coco_messages::Message;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::state::derive::tool_result_output;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;

pub(super) fn try_render(
    w: &ChatWidget<'_>,
    cell: &RenderedCell,
    lines: &mut Vec<Line<'static>>,
) -> Option<()> {
    let CellKind::ToolResult { .. } = &cell.kind else {
        return None;
    };
    let Message::ToolResult(tr) = cell.source.as_ref() else {
        return Some(());
    };
    let (tool_name, output) = tool_result_output(cell.source.as_ref())?;
    if tr.is_error {
        lines.push(Line::from(vec![
            Span::raw("  ● ").fg(w.styles.tool_error()),
            Span::raw(tool_name).fg(w.styles.text()).bold(),
            Span::raw(": ").fg(w.styles.dim()),
            Span::raw(output).fg(w.styles.error()),
        ]));
    } else {
        // Mirror the invocation row: the result block reuses the `●`
        // glyph so the eye groups call+result. Status is colour-encoded
        // (green ⇒ completed). Body is indented four spaces past the
        // gutter so column alignment matches the tool-use header.
        lines.push(Line::from(vec![
            Span::raw("  ● ").fg(w.styles.tool_completed()),
            Span::raw(tool_name).fg(w.styles.text()).bold(),
        ]));
        w.render_output_preview(&output, lines);
    }
    Some(())
}
