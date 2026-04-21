//! TS `queued_command` generator.
//!
//! Mirrors `normalizeAttachmentForAPI` `case 'queued_command':`
//! (`messages.ts:3739`). Replays drained queue items. Coco-rs uses a
//! simplified model: each `QueuedCommandInfo` carries its content +
//! an `origin_system` flag. System-origin items are wrapped in a
//! `<system-reminder>`; human-origin items pass through as plain user
//! text (TS hides system-generated from transcript via `isMeta`).
//!
//! This generator only emits the **system-origin** subset; human-
//! origin queued input is surfaced through the regular prompt
//! pipeline, not the reminder system.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

#[derive(Debug, Default)]
pub struct QueuedCommandGenerator;

#[async_trait]
impl AttachmentGenerator for QueuedCommandGenerator {
    fn name(&self) -> &str {
        "QueuedCommandGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::QueuedCommand
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.queued_command
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let mut parts: Vec<String> = Vec::new();
        for q in &ctx.queued_commands {
            if !q.origin_system {
                continue;
            }
            if q.content.is_empty() {
                continue;
            }
            parts.push(q.content.clone());
        }
        if parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::new(
            AttachmentType::QueuedCommand,
            parts.join("\n\n"),
        )))
    }
}

#[cfg(test)]
#[path = "queued_command.test.rs"]
mod tests;
