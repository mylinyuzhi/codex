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
            let mut iter = text.lines();
            for line in iter.by_ref() {
                lines.push(Line::from(
                    Span::raw(format!("  # {line}")).fg(w.styles.system_message()),
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
                .fg(w.styles.error()),
            ));
            Some(())
        }
        MessageContent::RateLimit { message, resets_at } => {
            let reset = resets_at
                .map(|t| format!(" (resets at {t})"))
                .unwrap_or_default();
            lines.push(Line::from(
                Span::raw(format!("  ⏱ {message}{reset}")).fg(w.styles.warning()),
            ));
            Some(())
        }
        MessageContent::Shutdown { reason } => {
            lines.push(Line::from(
                Span::raw(t!("chat.session_ended", reason = reason).to_string())
                    .fg(w.styles.dim())
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
                .fg(w.styles.error()),
            ));
            Some(())
        }
        MessageContent::ShutdownRejected { from, reason } => {
            lines.push(Line::from(
                Span::raw(t!("chat.shutdown_rejected", from = from, reason = reason).to_string())
                    .fg(w.styles.dim()),
            ));
            Some(())
        }
        MessageContent::HookSuccess { hook_name, output } => {
            lines.push(Line::from(vec![
                Span::raw("  ⚙ ").fg(w.styles.accent()),
                Span::raw(format!("{hook_name}: ")).dim(),
                Span::raw(output.clone()).green(),
            ]));
            Some(())
        }
        MessageContent::HookNonBlockingError { hook_name, error } => {
            lines.push(Line::from(vec![
                Span::raw("  ⚠ ").fg(w.styles.warning()),
                Span::raw(format!("{hook_name}: ")).fg(w.styles.dim()),
                Span::raw(error.clone()).fg(w.styles.warning()),
            ]));
            Some(())
        }
        MessageContent::HookBlockingError {
            hook_name,
            error,
            command,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  ✗ ").fg(w.styles.error()),
                Span::raw(format!("{hook_name}: ")).fg(w.styles.dim()),
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
                Span::raw(format!("  {hook_name}: ")).fg(w.styles.dim()),
                Span::raw(message.clone()).cyan(),
            ]));
            Some(())
        }
        MessageContent::HookAdditionalContext { hook_name, context } => {
            lines.push(Line::from(vec![
                Span::raw(format!("  {hook_name}: ")).dim(),
                Span::raw(context.clone()).fg(w.styles.text()),
            ]));
            Some(())
        }
        MessageContent::HookStoppedContinuation { hook_name, reason } => {
            lines.push(Line::from(vec![
                Span::raw(format!("  {hook_name}: ")).fg(w.styles.dim()),
                Span::raw(reason.clone()).fg(w.styles.warning()),
            ]));
            Some(())
        }
        MessageContent::HookAsyncResponse { hook_name, output } => {
            lines.push(Line::from(vec![
                Span::raw("  ⚙ ").fg(w.styles.accent()),
                Span::raw(format!("{hook_name}: ")).dim(),
                Span::raw(output.clone()).fg(w.styles.text()),
            ]));
            Some(())
        }
        MessageContent::PlanApproval { plan, .. } => {
            lines.push(Line::from(
                Span::raw(t!("chat.plan_for_review").to_string())
                    .fg(w.styles.plan())
                    .bold(),
            ));
            for line in plan.lines().take(20) {
                lines.push(Line::from(
                    Span::raw(format!("  │ {line}")).fg(w.styles.text()),
                ));
            }
            Some(())
        }
        MessageContent::CompactBoundary => {
            let border = "─".repeat(40);
            lines.push(Line::from(
                Span::raw(format!("  {border}")).fg(w.styles.border()).dim(),
            ));
            Some(())
        }
        MessageContent::CompactSummary {
            summary,
            messages_summarized,
            user_context,
            trigger,
        } => {
            // TS: components/CompactSummary.tsx
            let heading = match trigger {
                coco_types::CompactTrigger::SessionMemory => "Summarized via session memory",
                coco_types::CompactTrigger::Reactive => "Summarized (PTL recovery)",
                coco_types::CompactTrigger::TimeBased => "Summarized (idle gap)",
                coco_types::CompactTrigger::ContextCollapse => "Summarized (context collapse)",
                _ => "Conversation summary",
            };
            let mut hdr = vec![
                Span::raw("  ✻ ").fg(w.styles.accent()),
                Span::raw(heading).fg(w.styles.primary()).bold(),
            ];
            if let Some(n) = messages_summarized {
                hdr.push(Span::raw(format!(" · {n} messages")).fg(w.styles.dim()));
            }
            lines.push(Line::from(hdr));
            if let Some(ctx) = user_context.as_ref().filter(|s| !s.is_empty()) {
                lines.push(Line::from(
                    Span::raw(format!("    focus: {ctx}")).fg(w.styles.dim()),
                ));
            }
            for line in summary.lines().take(8) {
                lines.push(Line::from(
                    Span::raw(format!("    {line}")).fg(w.styles.text()),
                ));
            }
            if summary.lines().count() > 8 {
                // Render the actual user-bound shortcut for
                // `app:toggleTranscript` (defaults to `ctrl+o`) so
                // user customizations show through. Falls back to
                // the default literal when nothing's bound.
                let shortcut = w
                    .kb_handle
                    .and_then(|h| {
                        h.display_for(
                            &coco_keybindings::KeybindingAction::AppToggleTranscript,
                            crate::keybinding_bridge::KeybindingContext::Chat,
                        )
                    })
                    .unwrap_or_else(|| "ctrl+o".to_string());
                lines.push(Line::from(
                    Span::raw(format!("    …({shortcut} to see full summary)")).fg(w.styles.dim()),
                ));
            }
            Some(())
        }
        MessageContent::TaskAssignment {
            task_id,
            assignee,
            description,
        } => {
            lines.push(Line::from(vec![
                Span::raw("  📌 ").fg(w.styles.accent()),
                Span::raw(format!("Task {task_id} → @{assignee}: "))
                    .fg(w.styles.primary())
                    .bold(),
                Span::raw(description.clone()).fg(w.styles.text()),
            ]));
            Some(())
        }
        _ => None,
    }
}
