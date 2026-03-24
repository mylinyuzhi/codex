//! MCP instructions delta generator.
//!
//! Notifies the model when MCP server instructions have changed
//! (e.g., server reconnected with different instructions, new server added).

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for MCP instructions delta notifications.
#[derive(Debug)]
pub struct McpInstructionsDeltaGenerator;

#[async_trait]
impl AttachmentGenerator for McpInstructionsDeltaGenerator {
    fn name(&self) -> &str {
        "McpInstructionsDeltaGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::McpInstructionsDelta
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.mcp_instructions_delta
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let changes = &ctx.mcp_instructions_changes;
        if changes.is_empty() {
            return Ok(None);
        }

        let mut lines = vec!["MCP server instructions updated:".to_string()];
        for (server, instruction) in changes {
            lines.push(format!("- **{server}**: {instruction}"));
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::McpInstructionsDelta,
            lines.join("\n"),
        )))
    }
}

#[cfg(test)]
#[path = "mcp_instructions_delta.test.rs"]
mod tests;
