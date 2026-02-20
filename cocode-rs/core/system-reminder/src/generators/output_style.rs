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

/// Generator for output style reinforcement reminders.
///
/// Since the output style is now injected directly into the system prompt
/// (via `SystemPromptBuilder`), this generator serves as a periodic reinforcement
/// to remind the model to follow the active output style instructions.
///
/// It supports:
/// - Built-in styles: "explanatory" (educational insights) and "learning" (hands-on learning)
/// - Custom instruction text provided via configuration
///
/// The generator only runs for the main agent and uses moderate throttling
/// (every 15 turns) since the style is already in the system prompt.
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
        // Periodic reinforcement — the output style is already in the system prompt,
        // so we only need occasional reminders.
        ThrottleConfig::output_style()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        // Resolve the style name for the reinforcement message
        let style_name = ctx
            .config
            .output_style
            .style_name
            .as_deref()
            .unwrap_or("custom");

        let reminder = format!(
            "Remember: Follow the active output style \"{style_name}\" instructions in your system prompt."
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::OutputStyle,
            reminder,
        )))
    }
}

#[cfg(test)]
#[path = "output_style.test.rs"]
mod tests;
