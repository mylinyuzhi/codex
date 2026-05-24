//! Agent mentions generator.
//!
//! Injects instructions for @agent-* mentions in user prompts.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::parsing::parse_agent_mentions;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for @agent-* mentions.
///
/// Parses the user prompt for @agent-type mentions and generates
/// agent-specific instructions.
#[derive(Debug)]
pub struct AgentMentionsGenerator;

#[async_trait]
impl AttachmentGenerator for AgentMentionsGenerator {
    fn name(&self) -> &str {
        "AgentMentionsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AgentMentions
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::UserPrompt
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.agent_mentions
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let user_prompt = match ctx.user_prompt {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(None),
        };

        let mentions = parse_agent_mentions(user_prompt);
        if mentions.is_empty() {
            return Ok(None);
        }

        let mut content = String::new();
        for mention in &mentions {
            content.push_str(&format!(
                "The user has expressed a desire to invoke the agent \"{}\". \
                 Please invoke the agent appropriately, passing in the required context to it.\n\n",
                mention.agent_type
            ));
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::AgentMentions,
            content.trim(),
        )))
    }
}

#[cfg(test)]
#[path = "agent_mentions.test.rs"]
mod tests;
