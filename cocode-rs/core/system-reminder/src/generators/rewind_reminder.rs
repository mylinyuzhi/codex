use async_trait::async_trait;

use crate::Result;

use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::RewindMode;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generates a one-time system reminder when a rewind has occurred.
///
/// Only emitted for `CodeOnly` mode where the model's conversation continues
/// but file state has been reverted — the model needs to know its previous
/// edits were undone. For `CodeAndConversation` / `ConversationOnly`, the
/// conversation is truncated so the model has no memory of the rewound turns
/// and a reminder would be counterproductive (aligned with Claude Code behavior).
#[derive(Debug)]
pub struct RewindReminderGenerator;

#[async_trait]
impl AttachmentGenerator for RewindReminderGenerator {
    fn name(&self) -> &str {
        "RewindReminderGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::Rewind
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.rewind
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle — this is a one-time event consumed immediately.
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(ref info) = ctx.rewind_info else {
            return Ok(None);
        };

        // Only emit a reminder for CodeOnly mode. When conversation is
        // truncated (CodeAndConversation / ConversationOnly), the model
        // has no memory of the rewound turns — informing it would create
        // a context mismatch and waste tokens.
        if info.rewind_mode != RewindMode::CodeOnly {
            return Ok(None);
        }

        let git_note = if info.used_git_restore {
            " The git working tree was restored from a snapshot commit."
        } else {
            ""
        };

        let content = format!(
            "The user reverted file changes from turn {turn}. {files} file(s) were \
             restored to their pre-turn state.{git_note} Your previous edits from that \
             turn are no longer on disk. The conversation history is unchanged.",
            turn = info.rewound_turn_number,
            files = info.restored_file_count,
        );

        Ok(Some(SystemReminder::new(AttachmentType::Rewind, content)))
    }
}
