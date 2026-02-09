//! Unified tasks generator for background task status.
//!
//! Provides visibility into background tasks (shells, agents, remote sessions)
//! via system reminders. This generator is MainAgentOnly tier since subagents
//! don't need to know about background tasks from the main agent.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::BackgroundTaskInfo;
use crate::generator::BackgroundTaskStatus;
use crate::generator::BackgroundTaskType;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Key for unified task info in generator context extension data.
pub const UNIFIED_TASKS_KEY: &str = "unified_tasks";

/// Generator for background task status reminders.
///
/// This generator produces reminders that inform the model about
/// currently running background tasks, their status, and any recent
/// activity. This helps the model decide whether to check on tasks
/// or wait for them to complete.
#[derive(Debug)]
pub struct UnifiedTasksGenerator;

#[async_trait]
impl AttachmentGenerator for UnifiedTasksGenerator {
    fn name(&self) -> &str {
        "UnifiedTasksGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::BackgroundTask
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.background_task
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Check every turn since background task status can change rapidly
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Use background_tasks from context (already populated by the agent loop)
        if ctx.background_tasks.is_empty() {
            return Ok(None);
        }

        let content = format_tasks(&ctx.background_tasks);

        Ok(Some(SystemReminder::new(
            AttachmentType::BackgroundTask,
            content,
        )))
    }
}

/// Format background tasks for display in the system reminder.
fn format_tasks(tasks: &[BackgroundTaskInfo]) -> String {
    let mut content = String::new();
    content.push_str("## Background Tasks\n\n");

    // Group by status
    let running: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == BackgroundTaskStatus::Running)
        .collect();
    let completed: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == BackgroundTaskStatus::Completed)
        .collect();
    let failed: Vec<_> = tasks
        .iter()
        .filter(|t| t.status == BackgroundTaskStatus::Failed)
        .collect();

    if !running.is_empty() {
        content.push_str("### Running\n");
        for task in &running {
            content.push_str(&format_single_task(task));
        }
        content.push('\n');
    }

    if !completed.is_empty() {
        content.push_str("### Completed\n");
        for task in &completed {
            content.push_str(&format_single_task(task));
        }
        content.push('\n');
    }

    if !failed.is_empty() {
        content.push_str("### Failed\n");
        for task in &failed {
            content.push_str(&format_single_task(task));
        }
        content.push('\n');
    }

    // Add summary and hints
    content.push_str(&format!(
        "Total: {} running, {} completed, {} failed\n",
        running.len(),
        completed.len(),
        failed.len()
    ));

    if !running.is_empty() {
        content.push_str("\nUse `TaskOutput` tool to check on running tasks.");
    }

    content
}

/// Format a single task entry.
fn format_single_task(task: &BackgroundTaskInfo) -> String {
    let type_label = match task.task_type {
        BackgroundTaskType::Shell => "shell",
        BackgroundTaskType::AsyncAgent => "agent",
        BackgroundTaskType::RemoteSession => "remote",
    };

    let new_output_marker = if task.has_new_output {
        " (new output)"
    } else {
        ""
    };

    let exit_info = task
        .exit_code
        .map(|code| format!(" [exit: {code}]"))
        .unwrap_or_default();

    format!(
        "- [{type_label}] `{id}`: {cmd}{exit_info}{new_output_marker}\n",
        id = task.task_id,
        cmd = task.command,
    )
}

#[cfg(test)]
#[path = "unified_tasks.test.rs"]
mod tests;
