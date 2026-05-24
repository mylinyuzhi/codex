//! TS `todo_reminder` generator.
//!
//! Mirrors `getTodoReminderAttachments` (`attachments.ts:3266`) +
//! `normalizeAttachmentForAPI` `case 'todo_reminder':` (`messages.ts:3663`).
//!
//! Gate chain (all must pass):
//!
//! 1. `TodoWrite` tool is present in `ctx.tools`.
//! 2. `Brief` tool is **not** present (TS `BRIEF_TOOL_NAME` gate) —
//!    when Brief is the primary I/O channel TodoWrite becomes a side
//!    channel and nudging conflicts with the brief workflow.
//! 3. `turns_since_last_todo_write >= 10`
//!    (TS `TODO_REMINDER_CONFIG.TURNS_SINCE_WRITE`).
//! 4. `turns_since_last_todo_reminder >= 10`
//!    (TS `TODO_REMINDER_CONFIG.TURNS_BETWEEN_REMINDERS`).
//!
//! Content is the TS string literal at `messages.ts:3668`, optionally
//! followed by a bracketed list of the agent's current todos if any exist.

use async_trait::async_trait;
use coco_types::TodoRecord;
use coco_types::ToolName;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

/// Tools that gate the reminder. The canonical wire strings come from the
/// [`ToolName`] enum's `as_str()`, so renaming a variant flows through
/// automatically and there are no hand-written magic strings here.
const TRIGGER_TOOL: ToolName = ToolName::TodoWrite;
const SUPPRESS_TOOL: ToolName = ToolName::Brief;

/// TS thresholds from `TODO_REMINDER_CONFIG` (`attachments.ts:254-257`).
const TURNS_SINCE_WRITE: i32 = 10;
const TURNS_BETWEEN_REMINDERS: i32 = 10;

/// Verbatim message body from `messages.ts:3668` (sans trailing `\n` which
/// TS appends before optional list suffix; we inject it explicitly).
const TODO_REMINDER_BODY: &str = "The TodoWrite tool hasn't been used recently. If you're working on tasks that would benefit from tracking progress, consider using the TodoWrite tool to track progress. Also consider cleaning up the todo list if has become stale and no longer matches what you are working on. Only use it if it's relevant to the current work. This is just a gentle reminder - ignore if not applicable. Make sure that you NEVER mention this reminder to the user";

/// Nudge the agent to use `TodoWrite` after a long silence.
#[derive(Debug, Default)]
pub struct TodoRemindersGenerator;

#[async_trait]
impl AttachmentGenerator for TodoRemindersGenerator {
    fn name(&self) -> &str {
        "TodoRemindersGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TodoReminder
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.todo_reminder
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::todo_reminder()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // TS V2 takes precedence: when V2 is enabled, `TaskRemindersGenerator`
        // owns this turn. TS `attachments.ts:893-897` does the same switch.
        if ctx.is_task_v2_enabled {
            return Ok(None);
        }

        if !tools_contain(&ctx.tools, TRIGGER_TOOL) {
            return Ok(None);
        }

        if tools_contain(&ctx.tools, SUPPRESS_TOOL) {
            return Ok(None);
        }

        if ctx.turns_since_last_todo_write < TURNS_SINCE_WRITE
            || ctx.turns_since_last_todo_reminder < TURNS_BETWEEN_REMINDERS
        {
            return Ok(None);
        }

        let content = render_todo_reminder_body(&ctx.todos);
        Ok(Some(SystemReminder::new(
            AttachmentType::TodoReminder,
            content,
        )))
    }
}

/// True when `tools` (raw wire-strings including MCP / custom names)
/// contains the wire form of `builtin`.
fn tools_contain(tools: &[String], builtin: ToolName) -> bool {
    let target = builtin.as_str();
    tools.iter().any(|t| t == target)
}

fn render_todo_reminder_body(todos: &[TodoRecord]) -> String {
    let mut out = String::from(TODO_REMINDER_BODY);
    if todos.is_empty() {
        return out;
    }
    // TS: `${index + 1}. [${status}] ${content}` joined by `\n`, wrapped in
    // `[…]`. Preserve the outer brackets exactly — they're user-visible.
    let items = todos
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{n}. [{s}] {c}", n = i + 1, s = t.status, c = t.content))
        .collect::<Vec<_>>()
        .join("\n");
    out.push_str("\n\n\nHere are the existing contents of your todo list:\n\n[");
    out.push_str(&items);
    out.push(']');
    out
}

#[cfg(test)]
#[path = "todo_reminders.test.rs"]
mod tests;
