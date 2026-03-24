//! Session name generator.
//!
//! Injects the session name into context so the model knows which
//! session it is operating in.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for session name display.
#[derive(Debug)]
pub struct SessionNameGenerator;

#[async_trait]
impl AttachmentGenerator for SessionNameGenerator {
    fn name(&self) -> &str {
        "SessionNameGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::SessionName
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.session_name
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(ref name) = ctx.session_name else {
            return Ok(None);
        };

        if name.is_empty() {
            return Ok(None);
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::SessionName,
            format!("Current session name: \"{name}\""),
        )))
    }
}

#[cfg(test)]
#[path = "session_name.test.rs"]
mod tests;
