//! Configuration change generator.
//!
//! Notifies the model when runtime configuration has changed
//! (e.g., model switch, permission mode change, thinking level change).

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for configuration change notifications.
#[derive(Debug)]
pub struct ConfigChangeGenerator;

#[async_trait]
impl AttachmentGenerator for ConfigChangeGenerator {
    fn name(&self) -> &str {
        "ConfigChangeGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::ConfigChange
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.config_change
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let changes = &ctx.config_changes;
        if changes.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["Configuration changes:".to_string()];
        for change in changes {
            lines.push(format!("- {change}"));
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::ConfigChange,
            lines.join("\n"),
        )))
    }
}

#[cfg(test)]
#[path = "config_change.test.rs"]
mod tests;
