//! Output token usage generator.
//!
//! Reports output token consumption for the current turn to help
//! the model stay within output limits.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generators::token_usage::format_tokens;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for output token usage tracking.
#[derive(Debug)]
pub struct OutputTokenUsageGenerator;

#[async_trait]
impl AttachmentGenerator for OutputTokenUsageGenerator {
    fn name(&self) -> &str {
        "OutputTokenUsageGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::OutputTokenUsage
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.output_token_usage
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Report every 5 turns
        ThrottleConfig {
            min_turns_between: 5,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(usage) = &ctx.token_usage else {
            return Ok(None);
        };

        if usage.output_tokens <= 0 {
            return Ok(None);
        }

        let content = format!(
            "Output tokens this turn: {}",
            format_tokens(usage.output_tokens)
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::OutputTokenUsage,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "output_token_usage.test.rs"]
mod tests;
