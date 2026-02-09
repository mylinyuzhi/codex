//! Queued commands generator for real-time steering.
//!
//! This generator converts queued user commands (entered via Enter during streaming)
//! into system reminders that steer the model in real-time. Each command is consumed
//! once (consume-then-remove pattern) and wrapped as:
//!
//! ```text
//! The user sent the following message:
//! {prompt}
//!
//! Please address this message and continue with your tasks.
//! ```

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for queued commands (real-time steering).
///
/// When the user queues a command during streaming, this generator
/// converts it to a steering message that the model can use to
/// adjust its current response.
#[derive(Debug)]
pub struct QueuedCommandsGenerator;

#[async_trait]
impl AttachmentGenerator for QueuedCommandsGenerator {
    fn name(&self) -> &str {
        "QueuedCommandsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::QueuedCommands
    }

    fn is_enabled(&self, _config: &SystemReminderConfig) -> bool {
        // Always enabled - this is a core steering mechanism
        true
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle - always inject immediately for real-time steering
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.queued_commands.is_empty() {
            return Ok(None);
        }

        // Wrap each command with Claude Code's steering format that explicitly
        // asks the model to address the message and continue.
        let content = ctx
            .queued_commands
            .iter()
            .map(|cmd| {
                format!(
                    "The user sent the following message:\n{}\n\n\
                     Please address this message and continue with your tasks.",
                    cmd.prompt
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        Ok(Some(SystemReminder::new(
            AttachmentType::QueuedCommands,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "queued_commands.test.rs"]
mod tests;
