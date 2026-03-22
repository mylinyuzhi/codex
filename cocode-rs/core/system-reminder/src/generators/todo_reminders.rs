//! Todo/task reminders generator.
//!
//! Injects current task list context to help the agent track progress.
//! Supports both plain TodoWrite items and rich structured tasks from
//! TaskCreate/TaskUpdate tools.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::StructuredTaskInfo;
use crate::generator::TodoStatus;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for todo/task reminders.
#[derive(Debug)]
pub struct TodoRemindersGenerator;

#[async_trait]
impl AttachmentGenerator for TodoRemindersGenerator {
    fn name(&self) -> &str {
        "TodoRemindersGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TodoReminders
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.todo_reminders
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::todo_reminder()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.has_todos() {
            return Ok(None);
        }

        // Prefer structured tasks when available, fall back to plain todos
        let content = if ctx.has_structured_tasks() {
            format_structured_tasks(&ctx.structured_tasks)
        } else {
            format_plain_todos(ctx)
        };

        Ok(Some(SystemReminder::new(
            AttachmentType::TodoReminders,
            content,
        )))
    }
}

/// Format rich structured tasks for the system reminder.
fn format_structured_tasks(tasks: &[StructuredTaskInfo]) -> String {
    let mut content = String::new();
    content.push_str("## Current Tasks\n\n");

    let in_progress: Vec<_> = tasks.iter().filter(|t| t.status == "in_progress").collect();
    let pending: Vec<_> = tasks.iter().filter(|t| t.status == "pending").collect();
    let completed: Vec<_> = tasks.iter().filter(|t| t.status == "completed").collect();

    if !in_progress.is_empty() {
        content.push_str("### In Progress\n");
        for task in &in_progress {
            format_structured_task_line(&mut content, task);
        }
        content.push('\n');
    }

    if !pending.is_empty() {
        content.push_str("### Pending\n");
        for task in &pending {
            format_structured_task_line(&mut content, task);
        }
        content.push('\n');
    }

    // Summary
    let total = in_progress.len() + pending.len() + completed.len();
    content.push_str(&format!(
        "Progress: {}/{total} tasks completed\n",
        completed.len()
    ));

    content.push_str("\nUse TaskUpdate to mark tasks as in_progress or completed.");

    content
}

/// Format a single structured task as a rich reminder line.
fn format_structured_task_line(content: &mut String, task: &StructuredTaskInfo) {
    let marker = match task.status.as_str() {
        "completed" => "[x]",
        "in_progress" => "[>]",
        _ => "[ ]",
    };

    content.push_str(&format!("- {marker} {}: {}", task.id, task.subject));

    if let Some(ref owner) = task.owner {
        content.push_str(&format!(" | Owner: {owner}"));
    }

    if task.is_blocked {
        let blockers = task.blocked_by.join(", ");
        content.push_str(&format!(" | Blocked by: {blockers}"));
    }

    if !task.blocks.is_empty() {
        content.push_str(&format!(" | Blocks: {}", task.blocks.join(", ")));
    }

    content.push('\n');

    if let Some(ref desc) = task.description {
        content.push_str(&format!("  Description: {desc}\n"));
    }
}

/// Format plain TodoWrite items (backwards-compatible path).
fn format_plain_todos(ctx: &GeneratorContext<'_>) -> String {
    let mut content = String::new();
    content.push_str("## Current Tasks\n\n");

    let in_progress: Vec<_> = ctx.in_progress_todos().collect();
    let pending: Vec<_> = ctx.pending_todos().collect();

    if !in_progress.is_empty() {
        content.push_str("### In Progress\n");
        for task in &in_progress {
            let blocked = if task.is_blocked { " (blocked)" } else { "" };
            content.push_str(&format!("- [{}] {}{}\n", task.id, task.subject, blocked));
        }
        content.push('\n');
    }

    if !pending.is_empty() {
        content.push_str("### Pending\n");
        for task in &pending {
            let blocked = if task.is_blocked { " (blocked)" } else { "" };
            content.push_str(&format!("- [{}] {}{}\n", task.id, task.subject, blocked));
        }
        content.push('\n');
    }

    let completed_count = ctx
        .todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    let total = ctx.todos.len();

    content.push_str(&format!(
        "Progress: {completed_count}/{total} tasks completed\n"
    ));

    content.push_str("\nUse TaskUpdate to mark tasks as in_progress or completed.");

    content
}

#[cfg(test)]
#[path = "todo_reminders.test.rs"]
mod tests;
