//! Worktree state generator.
//!
//! Notifies the model about active git worktrees in the session,
//! including creation and removal events.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for worktree state notifications.
#[derive(Debug)]
pub struct WorktreeStateGenerator;

#[async_trait]
impl AttachmentGenerator for WorktreeStateGenerator {
    fn name(&self) -> &str {
        "WorktreeStateGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::WorktreeState
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.worktree_state
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let count = ctx.active_worktree_count;
        if count <= 0 {
            return Ok(None);
        }

        let content = format!(
            "There are currently {count} active git worktree(s) in this session. \
             Use `ExitWorktree` to clean up worktrees when done."
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::WorktreeState,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "worktree_state.test.rs"]
mod tests;
