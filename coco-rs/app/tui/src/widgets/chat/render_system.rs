//! System-cell renderers — informational rows, API errors, compaction
//! boundaries, tool-use summaries.
//!
//! Phase 3d (§6): dispatches on `cell.kind` / `cell.source`.
//! `MessageContent` variants that were never produced by the engine
//! flow (`RateLimit`, `Shutdown*`, `Hook*`, `PlanApproval`,
//! `CompactSummary`, `Advisor`, `TaskAssignment`) went away with the
//! projection — those code paths were unreachable in production after
//! Phase 3c moved system messages onto the engine `MessageHistory`.

use coco_messages::Message;
use coco_messages::SystemMessage;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use super::ChatWidget;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;
use crate::state::transcript_view::SystemCellKind;

pub(super) fn try_render(
    w: &ChatWidget<'_>,
    cell: &RenderedCell,
    lines: &mut Vec<Line<'static>>,
) -> Option<()> {
    match &cell.kind {
        CellKind::ToolUseSummary { summary } => {
            for line in summary.lines() {
                lines.push(Line::from(
                    Span::raw(format!("  # {line}")).fg(w.styles.system_message()),
                ));
            }
            Some(())
        }
        CellKind::System(SystemCellKind::Informational) => {
            let Message::System(SystemMessage::Informational(info)) = cell.source.as_ref() else {
                return Some(());
            };
            let body = if info.title.is_empty() {
                info.message.clone()
            } else {
                format!("{}: {}", info.title, info.message)
            };
            for line in body.lines() {
                lines.push(Line::from(
                    Span::raw(format!("  # {line}")).fg(w.styles.system_message()),
                ));
            }
            Some(())
        }
        CellKind::System(SystemCellKind::ApiError) => {
            let Message::System(SystemMessage::ApiError(e)) = cell.source.as_ref() else {
                return Some(());
            };
            let status = e.status_code.map(|c| format!(" [{c}]")).unwrap_or_default();
            lines.push(Line::from(
                Span::raw(format!(
                    "  ⚠{status} {error}",
                    status = status,
                    error = e.error
                ))
                .fg(w.styles.error()),
            ));
            Some(())
        }
        CellKind::System(SystemCellKind::CompactBoundary) => {
            let border = "─".repeat(40);
            lines.push(Line::from(
                Span::raw(format!("  {border}")).fg(w.styles.border()).dim(),
            ));
            Some(())
        }
        // Remaining SystemCellKind sub-variants (PermissionRetry,
        // BridgeStatus, MemorySaved, AwaySummary, AgentsKilled,
        // ApiMetrics, StopHookSummary, TurnDuration, ScheduledTaskFire,
        // MicrocompactBoundary) render as plain informational rows
        // using the wrapped engine message's text content.
        CellKind::System(_) => {
            let body = system_message_summary(cell.source.as_ref()).unwrap_or_default();
            if !body.is_empty() {
                for line in body.lines() {
                    lines.push(Line::from(
                        Span::raw(format!("  # {line}")).fg(w.styles.system_message()),
                    ));
                }
            }
            Some(())
        }
        _ => None,
    }
}

/// Best-effort short summary of a [`SystemMessage`] for the
/// generic fallback row. Each sub-variant has its own typed shape;
/// this helper picks the most readable string field.
fn system_message_summary(msg: &Message) -> Option<String> {
    let Message::System(sm) = msg else {
        return None;
    };
    Some(match sm {
        SystemMessage::PermissionRetry(m) => {
            format!("permission retry · {} · {}", m.tool_name, m.message)
        }
        SystemMessage::BridgeStatus(m) => match (m.connected, m.message.as_deref()) {
            (true, Some(msg)) => format!("bridge connected · {msg}"),
            (true, None) => "bridge connected".to_string(),
            (false, Some(msg)) => format!("bridge disconnected · {msg}"),
            (false, None) => "bridge disconnected".to_string(),
        },
        SystemMessage::MemorySaved(_) => "memory saved".to_string(),
        SystemMessage::AwaySummary(_) => "away summary".to_string(),
        SystemMessage::AgentsKilled(_) => "agents killed".to_string(),
        SystemMessage::ApiMetrics(_) => "API metrics".to_string(),
        SystemMessage::StopHookSummary(_) => "stop hook summary".to_string(),
        SystemMessage::TurnDuration(_) => "turn duration".to_string(),
        SystemMessage::ScheduledTaskFire(_) => "scheduled task".to_string(),
        // Handled by their own match arms above.
        SystemMessage::Informational(_)
        | SystemMessage::ApiError(_)
        | SystemMessage::CompactBoundary(_)
        | SystemMessage::MicrocompactBoundary(_)
        | SystemMessage::LocalCommand(_)
        | SystemMessage::UserInterruption(_) => return None,
    })
}
