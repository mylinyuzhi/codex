//! Background task status generator.
//!
//! Status of background shell tasks (P1).
//! Matches yH5() in Claude Code chunks.107.mjs:2419-2480.

use crate::config::system_reminder::SystemReminderConfig;
use crate::error::Result;
use crate::system_reminder::generator::{
    AttachmentGenerator, BackgroundTaskInfo, BackgroundTaskStatus, BackgroundTaskType,
    GeneratorContext,
};
use crate::system_reminder::throttle::ThrottleConfig;
use crate::system_reminder::types::{AttachmentType, ReminderTier, SystemReminder};
use async_trait::async_trait;

/// Background task status generator.
///
/// Generates notifications about background shell task status.
#[derive(Debug)]
pub struct BackgroundTaskGenerator;

impl BackgroundTaskGenerator {
    /// Create a new background task generator.
    pub fn new() -> Self {
        Self
    }

    /// Build the reminder content from task updates.
    fn build_content(&self, tasks: &[&BackgroundTaskInfo]) -> String {
        let mut content = String::new();

        for task in tasks {
            let mut parts = vec![format!("Background Bash {}", task.task_id)];

            if let Some(cmd) = &task.command {
                parts.push(format!("(command: {cmd})"));
            }

            let status_str = match task.status {
                BackgroundTaskStatus::Running => "running",
                BackgroundTaskStatus::Completed => "completed",
                BackgroundTaskStatus::Failed => "failed",
            };
            parts.push(format!("(status: {status_str})"));

            if let Some(code) = task.exit_code {
                parts.push(format!("(exit code: {code})"));
            }

            if task.has_new_output {
                parts.push(
                    "Has new output available. You can check its output using the BashOutput tool."
                        .to_string(),
                );
            }

            content.push_str(&parts.join(" "));
            content.push('\n');
        }

        content.trim_end().to_string()
    }
}

impl Default for BackgroundTaskGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AttachmentGenerator for BackgroundTaskGenerator {
    fn name(&self) -> &str {
        "background_task"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::BackgroundTask
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.enabled && config.attachments.background_task
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttling - immediate notification
        ThrottleConfig {
            min_turns_between: 0,
            min_turns_after_trigger: 0,
            max_per_session: None,
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Only for main agent
        if !ctx.is_main_agent {
            return Ok(None);
        }

        // Filter to shell tasks with updates
        let updates: Vec<_> = ctx
            .background_tasks
            .iter()
            .filter(|t| {
                t.task_type == BackgroundTaskType::Shell
                    && (t.has_new_output
                        || (t.status != BackgroundTaskStatus::Running && !t.notified))
            })
            .collect();

        if updates.is_empty() {
            return Ok(None);
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::BackgroundTask,
            self.build_content(&updates),
        )))
    }
}

// ============================================
// Tests
// ============================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_reminder::file_tracker::FileTracker;
    use crate::system_reminder::generator::TodoState;
    use std::path::Path;

    fn make_context<'a>(
        is_main_agent: bool,
        background_tasks: &'a [BackgroundTaskInfo],
        file_tracker: &'a FileTracker,
        todo_state: &'a TodoState,
    ) -> GeneratorContext<'a> {
        GeneratorContext {
            turn_number: 1,
            is_main_agent,
            has_user_input: true,
            cwd: Path::new("/test"),
            agent_id: "test-agent",
            file_tracker,
            is_plan_mode: false,
            plan_file_path: None,
            is_plan_reentry: false,
            todo_state,
            background_tasks,
            critical_instruction: None,
        }
    }

    #[tokio::test]
    async fn test_returns_none_for_subagent() {
        let generator = BackgroundTaskGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState::default();
        let tasks = vec![BackgroundTaskInfo {
            task_id: "task-1".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: Some("npm test".to_string()),
            description: "Running tests".to_string(),
            status: BackgroundTaskStatus::Completed,
            exit_code: Some(0),
            has_new_output: true,
            notified: false,
        }];
        let ctx = make_context(false, &tasks, &tracker, &todo_state);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_generates_for_main_agent_with_updates() {
        let generator = BackgroundTaskGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState::default();
        let tasks = vec![BackgroundTaskInfo {
            task_id: "task-1".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: Some("npm test".to_string()),
            description: "Running tests".to_string(),
            status: BackgroundTaskStatus::Completed,
            exit_code: Some(0),
            has_new_output: true,
            notified: false,
        }];
        let ctx = make_context(true, &tasks, &tracker, &todo_state);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_some());

        let reminder = result.unwrap();
        assert_eq!(reminder.attachment_type, AttachmentType::BackgroundTask);
        assert!(reminder.content.contains("task-1"));
        assert!(reminder.content.contains("npm test"));
        assert!(reminder.content.contains("completed"));
    }

    #[tokio::test]
    async fn test_returns_none_for_running_not_notified() {
        let generator = BackgroundTaskGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState::default();
        let tasks = vec![BackgroundTaskInfo {
            task_id: "task-1".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: Some("npm test".to_string()),
            description: "Running tests".to_string(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            has_new_output: false,
            notified: false,
        }];
        let ctx = make_context(true, &tasks, &tracker, &todo_state);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_includes_new_output_message() {
        let generator = BackgroundTaskGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState::default();
        let tasks = vec![BackgroundTaskInfo {
            task_id: "task-1".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: Some("npm test".to_string()),
            description: "Running tests".to_string(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            has_new_output: true,
            notified: false,
        }];
        let ctx = make_context(true, &tasks, &tracker, &todo_state);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_some());

        let reminder = result.unwrap();
        assert!(reminder.content.contains("new output available"));
    }

    #[test]
    fn test_main_agent_only_tier() {
        let generator = BackgroundTaskGenerator::new();
        assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);
    }

    #[test]
    fn test_attachment_type() {
        let generator = BackgroundTaskGenerator::new();
        assert_eq!(generator.attachment_type(), AttachmentType::BackgroundTask);
    }
}
