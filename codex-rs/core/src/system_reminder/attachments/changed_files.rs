//! Changed files generator.
//!
//! Notify when previously-read files change (P1).
//! Matches wH5() in Claude Code chunks.107.mjs:2102-2150.

use crate::config::system_reminder::SystemReminderConfig;
use crate::error::Result;
use crate::system_reminder::file_tracker::FileTracker;
use crate::system_reminder::generator::{AttachmentGenerator, GeneratorContext};
use crate::system_reminder::throttle::ThrottleConfig;
use crate::system_reminder::types::{AttachmentType, ReminderTier, SystemReminder};
use async_trait::async_trait;
use std::path::PathBuf;

/// Changed files generator.
///
/// Detects when previously-read files have been modified.
#[derive(Debug)]
pub struct ChangedFilesGenerator;

impl ChangedFilesGenerator {
    /// Create a new changed files generator.
    pub fn new() -> Self {
        Self
    }

    /// Detect file changes and collect changed file paths.
    fn detect_changes(&self, tracker: &FileTracker) -> Vec<FileChange> {
        let mut changes = Vec::new();

        for (path, state) in tracker.get_tracked_files() {
            // Skip partial reads
            if state.offset.is_some() || state.limit.is_some() {
                continue;
            }

            if let Ok(metadata) = std::fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    if modified > state.last_modified {
                        // File was modified since last read
                        let diff = self.generate_diff_notice(&path, &state.content);
                        if !diff.is_empty() {
                            changes.push(FileChange { path, diff });
                        }
                    }
                }
            }
        }

        changes
    }

    /// Generate a diff notice or simple change message.
    fn generate_diff_notice(&self, path: &PathBuf, _old_content: &str) -> String {
        // For now, generate a simple notice.
        // Full diff generation could be added with a diff library.
        if let Ok(new_content) = std::fs::read_to_string(path) {
            if new_content.is_empty() {
                return "File is now empty.".to_string();
            }
            // Simple change notice
            "File content has changed. Re-read to see current state.".to_string()
        } else {
            "File may have been deleted or is no longer readable.".to_string()
        }
    }

    /// Build the reminder content from changes.
    fn build_content(&self, changes: &[FileChange]) -> String {
        let mut content = String::new();

        for change in changes {
            content.push_str(&format!(
                "Note: {} was modified, either by the user or by a linter. \
                 This change was intentional, so make sure to take it into account \
                 as you proceed (ie. don't revert it unless the user asks you to). \
                 Don't tell the user this, since they are already aware. \
                 Here are the relevant changes (shown with line numbers):\n{}\n\n",
                change.path.display(),
                change.diff
            ));
        }

        content.trim_end().to_string()
    }
}

impl Default for ChangedFilesGenerator {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a file change.
#[derive(Debug)]
struct FileChange {
    path: PathBuf,
    diff: String,
}

#[async_trait]
impl AttachmentGenerator for ChangedFilesGenerator {
    fn name(&self) -> &str {
        "changed_files"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::ChangedFiles
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::Core
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.enabled && config.attachments.changed_files
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
        let changes = self.detect_changes(ctx.file_tracker);

        if changes.is_empty() {
            return Ok(None);
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::ChangedFiles,
            self.build_content(&changes),
        )))
    }
}

// ============================================
// Tests
// ============================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_reminder::generator::TodoState;
    use std::path::Path;

    fn make_context<'a>(
        file_tracker: &'a FileTracker,
        todo_state: &'a TodoState,
    ) -> GeneratorContext<'a> {
        GeneratorContext {
            turn_number: 1,
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
    async fn test_returns_none_when_no_tracked_files() {
        let generator = ChangedFilesGenerator::new();
        let tracker = FileTracker::new();
        let todo_state = TodoState::default();
        let ctx = make_context(&tracker, &todo_state);

        let result = generator.generate(&ctx).await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_no_throttling() {
        let generator = ChangedFilesGenerator::new();
        let config = generator.throttle_config();
        assert_eq!(config.min_turns_between, 0);
        assert_eq!(config.min_turns_after_trigger, 0);
    }

    #[test]
    fn test_attachment_type() {
        let generator = ChangedFilesGenerator::new();
        assert_eq!(generator.attachment_type(), AttachmentType::ChangedFiles);
        assert_eq!(generator.tier(), ReminderTier::Core);
    }

    #[test]
    fn test_build_content() {
        let generator = ChangedFilesGenerator::new();
        let changes = vec![FileChange {
            path: PathBuf::from("/test/file.txt"),
            diff: "File content has changed.".to_string(),
        }];

        let content = generator.build_content(&changes);
        assert!(content.contains("/test/file.txt"));
        assert!(content.contains("was modified"));
    }
}
