//! Date change reminder generator.
//!
//! Detects date rollover during long sessions and injects a reminder
//! with the new date, instructing the LLM not to mention it explicitly.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

#[derive(Debug)]
pub struct DateChangeGenerator;

#[async_trait]
impl AttachmentGenerator for DateChangeGenerator {
    fn name(&self) -> &str {
        "DateChangeGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::DateChange
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.date_change
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();

        let Some(ref last_date) = ctx.last_recorded_date else {
            // First turn: nothing to compare against
            return Ok(None);
        };

        if *last_date == today {
            return Ok(None);
        }

        let content = format!(
            "The date has changed. Today's date is now {today}. \
             DO NOT mention this to the user explicitly because they are already aware."
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::DateChange,
            content,
        )))
    }
}
