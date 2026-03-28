//! Effort level reminder generator.
//!
//! Injects per-turn reasoning effort level when extended thinking
//! is active, matching Claude Code's ultrathink_effort attachment.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

#[derive(Debug)]
pub struct EffortLevelGenerator;

#[async_trait]
impl AttachmentGenerator for EffortLevelGenerator {
    fn name(&self) -> &str {
        "EffortLevelGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::EffortLevel
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.effort_level
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(effort) = ctx.thinking_effort else {
            return Ok(None);
        };

        use cocode_protocol::model::ReasoningEffort;
        if effort < ReasoningEffort::High {
            return Ok(None);
        }

        let content = format!(
            "The user has requested reasoning effort level: {effort}. \
             Apply this to the current turn."
        );

        Ok(Some(SystemReminder::text(
            AttachmentType::EffortLevel,
            content,
        )))
    }
}
