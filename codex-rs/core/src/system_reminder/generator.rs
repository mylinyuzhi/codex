//! Attachment generator trait and context.
//!
//! Defines the interface for generating system reminder attachments.

use super::file_tracker::FileTracker;
use super::throttle::ThrottleConfig;
use super::types::{AttachmentType, ReminderTier, SystemReminder};
use crate::config::system_reminder::SystemReminderConfig;
use crate::error::Result;
use async_trait::async_trait;
use std::path::Path;

// ============================================
// Generator Trait
// ============================================

/// Trait for attachment generators.
///
/// Matches structure of individual generator functions in Claude Code.
#[async_trait]
pub trait AttachmentGenerator: Send + Sync + std::fmt::Debug {
    /// Unique name for this generator (for telemetry).
    fn name(&self) -> &str;

    /// Type of attachment this generator produces.
    fn attachment_type(&self) -> AttachmentType;

    /// Which tier this generator belongs to.
    fn tier(&self) -> ReminderTier {
        self.attachment_type().tier()
    }

    /// Generate attachment if applicable, returns None if not applicable this turn.
    /// This is the main entry point, called by orchestrator.
    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>>;

    /// Check if generator is enabled based on config.
    fn is_enabled(&self, config: &SystemReminderConfig) -> bool;

    /// Get throttle configuration for this generator.
    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::default()
    }
}

// ============================================
// Generator Context
// ============================================

/// Context provided to attachment generators.
///
/// Matches context parameter in Claude Code's generator functions.
#[derive(Debug)]
pub struct GeneratorContext<'a> {
    /// Current turn number in the conversation.
    pub turn_number: i32,
    /// Whether this is the main agent (not a sub-agent).
    pub is_main_agent: bool,
    /// Whether this turn has user input.
    pub has_user_input: bool,
    /// Current working directory.
    pub cwd: &'a Path,
    /// Session/Agent ID.
    pub agent_id: &'a str,
    /// File tracking state (for change detection).
    pub file_tracker: &'a FileTracker,
    /// Whether plan mode is active.
    pub is_plan_mode: bool,
    /// Plan file path (if in plan mode).
    pub plan_file_path: Option<&'a str>,
    /// Whether re-entering plan mode.
    pub is_plan_reentry: bool,
    /// Current todo list state.
    pub todo_state: &'a TodoState,
    /// Background task status.
    pub background_tasks: &'a [BackgroundTaskInfo],
    /// Critical instruction from config.
    pub critical_instruction: Option<&'a str>,
}

// ============================================
// Supporting Types
// ============================================

/// Current state of the todo list.
#[derive(Debug, Clone)]
pub struct TodoState {
    /// Whether the todo list is empty.
    pub is_empty: bool,
    /// Turn number when todo was last written.
    pub last_write_turn: i32,
    /// Current todo items.
    pub items: Vec<TodoItem>,
}

impl Default for TodoState {
    fn default() -> Self {
        Self {
            is_empty: true,
            last_write_turn: 0,
            items: vec![],
        }
    }
}

/// A single todo item.
#[derive(Debug, Clone)]
pub struct TodoItem {
    /// Content/description of the todo.
    pub content: String,
    /// Status: "pending", "in_progress", "completed".
    pub status: String,
    /// Active form of the content (for display).
    pub active_form: String,
}

/// Information about a background task.
#[derive(Debug, Clone)]
pub struct BackgroundTaskInfo {
    /// Unique task identifier.
    pub task_id: String,
    /// Type of background task.
    pub task_type: BackgroundTaskType,
    /// Command being executed (for shell tasks).
    pub command: Option<String>,
    /// Human-readable description.
    pub description: String,
    /// Current status.
    pub status: BackgroundTaskStatus,
    /// Exit code (if completed).
    pub exit_code: Option<i32>,
    /// Whether there's new output available.
    pub has_new_output: bool,
    /// Whether completion has been notified.
    pub notified: bool,
}

/// Type of background task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundTaskType {
    /// Background shell command.
    Shell,
    /// Async agent execution.
    AsyncAgent,
    /// Remote session.
    RemoteSession,
}

/// Status of a background task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundTaskStatus {
    /// Task is currently running.
    Running,
    /// Task completed successfully.
    Completed,
    /// Task failed.
    Failed,
}

// ============================================
// Tests
// ============================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_todo_state_default() {
        let state = TodoState::default();
        assert!(state.is_empty);
        assert_eq!(state.last_write_turn, 0);
        assert!(state.items.is_empty());
    }

    #[test]
    fn test_background_task_info() {
        let task = BackgroundTaskInfo {
            task_id: "task-1".to_string(),
            task_type: BackgroundTaskType::Shell,
            command: Some("npm test".to_string()),
            description: "Running tests".to_string(),
            status: BackgroundTaskStatus::Running,
            exit_code: None,
            has_new_output: true,
            notified: false,
        };

        assert_eq!(task.task_type, BackgroundTaskType::Shell);
        assert_eq!(task.status, BackgroundTaskStatus::Running);
        assert!(task.has_new_output);
    }

    #[test]
    fn test_background_task_status_equality() {
        assert_eq!(BackgroundTaskStatus::Running, BackgroundTaskStatus::Running);
        assert_ne!(
            BackgroundTaskStatus::Running,
            BackgroundTaskStatus::Completed
        );
    }
}
