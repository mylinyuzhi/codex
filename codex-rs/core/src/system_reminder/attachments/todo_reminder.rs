//! Todo reminder generator.
//!
//! Periodic reminder about empty/stale todo list (P0).
//! Matches _H5() in Claude Code chunks.107.mjs:2379-2394.

use crate::config::system_reminder::SystemReminderConfig;
use crate::error::Result;
use crate::system_reminder::generator::{AttachmentGenerator, GeneratorContext, TodoState};
use crate::system_reminder::throttle::{default_throttle_config, ThrottleConfig};
use crate::system_reminder::types::{AttachmentType, ReminderTier, SystemReminder};
use async_trait::async_trait;

/// Todo reminder generator.
///
/// Generates periodic reminders about using the TodoWrite tool.
#[derive(Debug)]
pub struct TodoReminderGenerator;

impl TodoReminderGenerator {
    /// Create a new todo reminder generator.
    pub fn new() -> Self {
        Self
    }

    /// Build reminder content matching Claude Code format.
    fn build_content(&self, todo_state: &TodoState) -> String {
        let mut message = String::from(
            "The TodoWrite tool hasn't been used recently. If you're working on tasks \
             that would benefit from tracking progress, consider using the TodoWrite tool \
             to track progress. Also consider cleaning up the todo list if has become \
             stale and no longer matches what you are working on. Only use it if it's \
             relevant to the current work. This is just a gentle reminder - ignore if \
             not applicable. Make sure that you NEVER mention this reminder to the user\n",
        );

        if !todo_state.items.is_empty() {
            let formatted_list: String = todo_state
                .items
                .iter()
                .enumerate()
                .map(|(i, item)| format!("{}. [{}] {}", i + 1, item.status, item.content))
                .collect::<Vec<_>>()
                .join("\n");

            message.push_str(&format!(
                "\n\nHere are the existing contents of your todo list:\n\n[{formatted_list}]"
            ));
        }

        message
    }
}

impl Default for TodoReminderGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AttachmentGenerator for TodoReminderGenerator {
    fn name(&self) -> &str {
        "todo_reminder"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TodoReminder
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::Core
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.enabled && config.attachments.todo_reminder
    }

    fn throttle_config(&self) -> ThrottleConfig {
        default_throttle_config(AttachmentType::TodoReminder)
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Additional trigger check: min turns since last TodoWrite
        // This is checked even if throttle passes
        let turns_since_write = ctx.turn_number.saturating_sub(ctx.todo_state.last_write_turn);
        if turns_since_write < 5 {
            // GY2.TURNS_SINCE_WRITE
            return Ok(None);
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::TodoReminder,
            self.build_content(ctx.todo_state),
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
    use crate::system_reminder::generator::TodoItem;
    use std::path::Path;

    fn make_context<'a>(
        turn_number: i32,
        todo_state: &'a TodoState,
        file_tracker: &'a FileTracker,
    ) -> GeneratorContext<'a> {
        GeneratorContext {
            turn_number,
            is_main_agent: true,
            has_user_input: true,
            cwd: Path::new("/test"),
            agent_id: "test-agent",
            file_tracker,
            is_plan_mode: false,
            plan_file_path: None,
            is_plan_reentry: false,
            todo_state,
            background_tasks: &[],
            critical_instruction: None,
        }
    }

    #[tokio::test]
    async fn test_generates_after_turns_since_write() {
        let generator = TodoReminderGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState {
            is_empty: true,
            last_write_turn: 1,
            items: vec![],
        };
        // Turn 7: 7 - 1 = 6 >= 5
        let ctx = make_context(7, &todo_state, &tracker);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_some());

        let reminder = result.unwrap();
        assert_eq!(reminder.attachment_type, AttachmentType::TodoReminder);
        assert!(reminder.content.contains("TodoWrite tool"));
    }

    #[tokio::test]
    async fn test_returns_none_when_recent_write() {
        let generator = TodoReminderGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState {
            is_empty: true,
            last_write_turn: 3,
            items: vec![],
        };
        // Turn 5: 5 - 3 = 2 < 5
        let ctx = make_context(5, &todo_state, &tracker);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_includes_todo_items() {
        let generator = TodoReminderGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState {
            is_empty: false,
            last_write_turn: 1,
            items: vec![
                TodoItem {
                    content: "First task".to_string(),
                    status: "pending".to_string(),
                    active_form: "Working on first task".to_string(),
                },
                TodoItem {
                    content: "Second task".to_string(),
                    status: "completed".to_string(),
                    active_form: "Working on second task".to_string(),
                },
            ],
        };
        let ctx = make_context(10, &todo_state, &tracker);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_some());

        let reminder = result.unwrap();
        assert!(reminder.content.contains("First task"));
        assert!(reminder.content.contains("Second task"));
        assert!(reminder.content.contains("[pending]"));
        assert!(reminder.content.contains("[completed]"));
    }

    #[test]
    fn test_throttle_config() {
        let generator = TodoReminderGenerator::new();
        let config = generator.throttle_config();
        assert_eq!(config.min_turns_between, 3);
        assert_eq!(config.min_turns_after_trigger, 5);
    }

    #[test]
    fn test_attachment_type() {
        let generator = TodoReminderGenerator::new();
        assert_eq!(generator.attachment_type(), AttachmentType::TodoReminder);
        assert_eq!(generator.tier(), ReminderTier::Core);
    }
}
