//! TS `critical_system_reminder` generator.
//!
//! Mirrors `getCriticalSystemReminderAttachment` (`attachments.ts:1587`) +
//! `normalizeAttachmentForAPI` `case 'critical_system_reminder':`
//! (`messages.ts:3872`). Emits a user-supplied instruction verbatim on
//! every turn while `config.critical_instruction` is non-empty.
//!
//! No cadence / no throttle: critical means "I want this in the model's
//! context every turn until I clear it." The caller is responsible for
//! removing the setting when no longer needed.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

/// Inject `config.critical_instruction` as a `<system-reminder>` every turn.
#[derive(Debug, Default)]
pub struct CriticalSystemReminderGenerator;

#[async_trait]
impl AttachmentGenerator for CriticalSystemReminderGenerator {
    fn name(&self) -> &str {
        "CriticalSystemReminderGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::CriticalSystemReminder
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.critical_system_reminder
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(instruction) = ctx.config.critical_instruction.as_deref() else {
            return Ok(None);
        };
        // Trim to treat whitespace-only as "not set". TS's equivalent reads
        // `toolUseContext.criticalSystemReminder_EXPERIMENTAL` and short-
        // circuits on missing/empty.
        if instruction.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::new(
            AttachmentType::CriticalSystemReminder,
            instruction.to_string(),
        )))
    }
}

#[cfg(test)]
#[path = "critical_system_reminder.test.rs"]
mod tests;
