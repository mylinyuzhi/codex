//! Deferred tools delta generator.
//!
//! Notifies the model when the set of available deferred tools has changed
//! since the last turn (e.g., new MCP tools loaded, tools removed).

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for deferred tools delta notifications.
#[derive(Debug)]
pub struct DeferredToolsDeltaGenerator;

#[async_trait]
impl AttachmentGenerator for DeferredToolsDeltaGenerator {
    fn name(&self) -> &str {
        "DeferredToolsDeltaGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::DeferredToolsDelta
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.deferred_tools_delta
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let added = &ctx.deferred_tools_added;
        let removed = &ctx.deferred_tools_removed;

        if added.is_empty() && removed.is_empty() {
            return Ok(None);
        }

        let mut lines = Vec::new();

        if !added.is_empty() {
            lines.push(format!(
                "New deferred tools available: {}",
                added.join(", ")
            ));
        }
        if !removed.is_empty() {
            lines.push(format!("Deferred tools removed: {}", removed.join(", ")));
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::DeferredToolsDelta,
            lines.join("\n"),
        )))
    }
}

#[cfg(test)]
#[path = "deferred_tools_delta.test.rs"]
mod tests;
