//! System-message renderers — plain system text, API errors, rate limits,
//! session shutdown signals, hook progress/errors, plan approvals,
//! compact boundaries, task assignments.

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
        MessageContent::SystemText(text) => {
            for line in text.lines() {
                lines.push(Line::from(
                    Span::raw(format!("  # {line}")).fg(w.theme.system_message),
                ));
            }
            Some(())
        }
        MessageContent::ApiError {
            error,
            retryable,
            status_code,
        } => {
            let status = status_code.map(|c| format!(" [{c}]")).unwrap_or_default();
            let retry = if *retryable { " (retrying...)" } else { "" };
            lines.push(Line::from(
                Span::raw(format!(
                    "  ⚠ {}",
                    t!(
                        "toast.api_error",
                        status = status,
                        error = error,
                        retry = retry
                    )
                ))
                .fg(w.theme.error),
            ));
            Some(())
        }
        MessageContent::RateLimit { message, resets_at } => {
            let reset = resets_at
                .map(|t| format!(" (resets at {t})"))
                .unwrap_or_default();
            lines.push(Line::from(
                Span::raw(format!("  ⏱ {message}{reset}")).fg(w.theme.warning),
            ));
            Some(())
        }
        MessageContent::Shutdown { reason } => {
            lines.push(Line::from(
                Span::raw(t!("chat.session_ended", reason = reason).to_string())
                    .fg(w.theme.text_dim)
                    .italic(),
            ));
            Some(())
        }
        MessageContent::ShutdownRequest { from, reason } => {
            let reason_text = reason
                .as_deref()
                .map(|r| format!(": {r}"))
                .unwrap_or_default();
            lines.push(Line::from(
                Span::raw(
                    t!("chat.shutdown_requested", from = from, reason = reason_text).to_string(),
                )
                .fg(w.theme.error),
            ));
            Some(())
        }
        MessageContent::ShutdownRejected { from, reason } => {
            lines.push(Line::from(
                Span::raw(t!("chat.shutdown_rejected", from = from, reason = reason).to_string())
                    .fg(w.theme.text_dim),
            ));
            Some(())
        }
        MessageContent::HookSuccess { hook_name, output } => {
            lines.push(Line::from(vec![
                Span::raw("  ⚙ ").fg(w.theme.accent),
                Span::raw(format!("{hook_name}: ")).dim(),
                Span::raw(output.clone()).green(),
            ]));
            Some(())
        }
        MessageContent::HookNonBlockingError { hook_name, error } => {
            lines.push(Line::from(vec![
                Span::raw("  ⚠ ").fg(w.theme.warning),
                Span::raw(format!("{hook_name}: ")).fg(w.theme.text_dim),
                Span::raw(error.clone()).yellow(),
            ]));
            Some(())
        }
        MessageContent::HookBlockingError {
            hook_name,
            error,
            command,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  ✗ ").fg(w.theme.error),
                Span::raw(format!("{hook_name}: ")).fg(w.theme.text_dim),
                Span::raw(error.clone()).red(),
            ]));
            lines.push(Line::from(
                Span::raw(format!("    command: {command}")).dim(),
            ));
            Some(())
        }
        MessageContent::HookCancelled { hook_name } => {
            lines.push(Line::from(vec![
                Span::raw(format!("  {hook_name}: ")).dim(),
                Span::raw(t!("chat.cancelled").to_string()).dim(),
            ]));
            Some(())
        }
        MessageContent::HookSystemMessage { hook_name, message } => {
            lines.push(Line::from(vec![
                Span::raw(format!("  {hook_name}: ")).fg(w.theme.text_dim),
                Span::raw(message.clone()).cyan(),
            ]));
            Some(())
        }
        MessageContent::HookAdditionalContext { hook_name, context } => {
            lines.push(Line::from(vec![
                Span::raw(format!("  {hook_name}: ")).dim(),
                Span::raw(context.clone()).fg(w.theme.text),
            ]));
            Some(())
        }
        MessageContent::HookStoppedContinuation { hook_name, reason } => {
            lines.push(Line::from(vec![
                Span::raw(format!("  {hook_name}: ")).fg(w.theme.text_dim),
                Span::raw(reason.clone()).yellow(),
            ]));
            Some(())
        }
        MessageContent::HookAsyncResponse { hook_name, output } => {
            lines.push(Line::from(vec![
                Span::raw("  ⚙ ").fg(w.theme.accent),
                Span::raw(format!("{hook_name}: ")).dim(),
                Span::raw(output.clone()).fg(w.theme.text),
            ]));
            Some(())
        }
        MessageContent::PlanApproval { plan, .. } => {
            lines.push(Line::from(
                Span::raw(t!("chat.plan_for_review").to_string())
                    .fg(w.theme.plan_mode)
                    .bold(),
            ));
            for line in plan.lines().take(20) {
                lines.push(Line::from(
                    Span::raw(format!("  │ {line}")).fg(w.theme.text),
                ));
            }
            Some(())
        }
        MessageContent::CompactBoundary => {
            let border = "─".repeat(40);
            lines.push(Line::from(
                Span::raw(format!("  {border}")).fg(w.theme.border).dim(),
            ));
            Some(())
        }
        MessageContent::TaskAssignment {
            task_id,
            assignee,
            description,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  📌 ").fg(w.theme.accent),
                Span::raw(format!("Task {task_id} → @{assignee}: "))
                    .fg(w.theme.primary)
                    .bold(),
                Span::raw(description.clone()).fg(w.theme.text),
            ]));
            Some(())
        }
        _ => None,
    }
}
