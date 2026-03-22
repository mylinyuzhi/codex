//! Compaction reminder generator.
//!
//! Injects a reminder that auto-compact is enabled, preventing "context anxiety"
//! where the model rushes to complete work or warns about running out of context.
//! Matches Claude Code's auto-compact system prompt injection.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for the compaction reminder system prompt.
///
/// When auto-compact is enabled, injects a message telling the model that
/// context will be automatically managed, so it should not rush or warn
/// about running out of context.
#[derive(Debug)]
pub struct CompactionReminderGenerator;

#[async_trait]
impl AttachmentGenerator for CompactionReminderGenerator {
    fn name(&self) -> &str {
        "CompactionReminderGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::CompactionReminder
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.compaction_reminder
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Always inject when auto-compact is enabled. After compaction clears
        // context, the model needs to see this reminder again.
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_auto_compact_enabled {
            return Ok(None);
        }

        Ok(Some(SystemReminder::text(
            AttachmentType::CompactionReminder,
            "Auto-compact is enabled. When the context window is nearly full, older messages \
             will be automatically summarized so you can continue working seamlessly. There is \
             no need to stop or rush \u{2014} you have unlimited context through automatic compaction.",
        )))
    }
}

#[cfg(test)]
#[path = "compaction_reminder.test.rs"]
mod tests;
