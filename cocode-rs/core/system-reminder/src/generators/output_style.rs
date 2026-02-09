//! Output style generator.
//!
//! Injects output style instructions to modify model response behavior.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for output style instructions.
///
/// This generator injects output style instructions that modify how the model
/// responds. It supports:
/// - Built-in styles: "explanatory" (educational insights) and "learning" (hands-on learning)
/// - Custom instruction text provided via configuration
///
/// The generator only runs for the main agent and uses high throttling (once per 50 turns)
/// since the style should remain consistent throughout a session.
#[derive(Debug)]
pub struct OutputStyleGenerator;

#[async_trait]
impl AttachmentGenerator for OutputStyleGenerator {
    fn name(&self) -> &str {
        "OutputStyleGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::OutputStyle
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.output_style
            && config.output_style.enabled
            && config.output_style.resolve_instruction().is_some()
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Output style injects once per session at the start,
        // consistent with Claude Code behavior
        ThrottleConfig::output_style()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Resolve the output style instruction from config
        let instruction = match ctx.config.output_style.resolve_instruction() {
            Some(i) => i,
            None => return Ok(None),
        };

        Ok(Some(SystemReminder::new(
            AttachmentType::OutputStyle,
            instruction,
        )))
    }
}

#[cfg(test)]
#[path = "output_style.test.rs"]
mod tests;
