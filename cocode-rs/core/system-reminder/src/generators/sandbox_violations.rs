//! Sandbox violation system reminder generator.
//!
//! Surfaces recent sandbox violations to the model so it can adjust
//! its behavior (e.g., avoid denied file paths or network access).

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for sandbox violation notifications.
///
/// Formats recent sandbox violations as an XML block so the model
/// can see which operations were denied and adjust accordingly.
#[derive(Debug)]
pub struct SandboxViolationsGenerator;

#[async_trait]
impl AttachmentGenerator for SandboxViolationsGenerator {
    fn name(&self) -> &str {
        "sandbox_violations"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::SandboxViolations
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.sandbox_violations
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle -- always surface violations so the model can react.
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.sandbox_violations.is_empty() {
            return Ok(None);
        }

        let count = ctx.sandbox_violations.len();
        let mut lines = Vec::with_capacity(count + 2);

        lines.push(format!(
            "<sandbox-violations>{count} violation(s) detected:"
        ));

        for (operation, path, command_tag) in &ctx.sandbox_violations {
            let mut entry = format!("- {operation}");
            if let Some(p) = path {
                entry.push_str(&format!(" path={p}"));
            }
            if let Some(tag) = command_tag {
                entry.push_str(&format!(" cmd={tag}"));
            }
            lines.push(entry);
        }

        lines.push("</sandbox-violations>".to_string());

        Ok(Some(SystemReminder::new(
            AttachmentType::SandboxViolations,
            lines.join("\n"),
        )))
    }
}

#[cfg(test)]
#[path = "sandbox_violations.test.rs"]
mod tests;
