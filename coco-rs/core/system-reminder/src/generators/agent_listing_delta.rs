//! TS `agent_listing_delta` generator.
//!
//! Mirrors `getAgentListingDeltaAttachment` (`attachments.ts:1490`) +
//! `normalizeAttachmentForAPI` `case 'agent_listing_delta':`
//! (`messages.ts:4194`). Announces agent-type additions / removals for
//! the Agent tool.
//!
//! Gate chain:
//!
//! 1. `ctx.config.attachments.agent_listing_delta` — default on.
//! 2. `ctx.agent_listing_delta.is_some()` with non-empty delta —
//!    engine pre-computes by diffing active agents against prior
//!    announcements in history.
//!
//! Text template from TS `messages.ts:4194-4214`: three optional
//! sections joined by `"\n\n"`:
//! - Added (header depends on `is_initial`).
//! - Removed agent types.
//! - Concurrency note (only on `is_initial && show_concurrency_note`).

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AgentListingDeltaInfo;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

#[derive(Debug, Default)]
pub struct AgentListingDeltaGenerator;

#[async_trait]
impl AttachmentGenerator for AgentListingDeltaGenerator {
    fn name(&self) -> &str {
        "AgentListingDeltaGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AgentListingDelta
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.agent_listing_delta
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(info) = ctx.agent_listing_delta.as_ref() else {
            return Ok(None);
        };
        if info.is_empty() && !info.show_concurrency_note {
            return Ok(None);
        }
        Ok(Some(SystemReminder::new(
            AttachmentType::AgentListingDelta,
            render(info),
        )))
    }
}

fn render(info: &AgentListingDeltaInfo) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(3);
    if !info.added_lines.is_empty() {
        let header = if info.is_initial {
            "Available agent types for the Agent tool:"
        } else {
            "New agent types are now available for the Agent tool:"
        };
        parts.push(format!("{header}\n{}", info.added_lines.join("\n")));
    }
    if !info.removed_types.is_empty() {
        let list: Vec<String> = info
            .removed_types
            .iter()
            .map(|t| format!("- {t}"))
            .collect();
        parts.push(format!(
            "The following agent types are no longer available:\n{}",
            list.join("\n")
        ));
    }
    if info.is_initial && info.show_concurrency_note {
        parts.push(
            "Launch multiple agents concurrently whenever possible, to maximize performance; to do that, use a single message with multiple tool uses."
                .to_string(),
        );
    }
    parts.join("\n\n")
}

#[cfg(test)]
#[path = "agent_listing_delta.test.rs"]
mod tests;
