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
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_config() -> SystemReminderConfig {
        SystemReminderConfig::default()
    }

    #[tokio::test]
    async fn test_no_mentions() {
        let config = test_config();
        let ctx = GeneratorContext::builder()
            .config(&config)
            .turn_number(1)
            .is_main_agent(true)
            .has_user_input(true)
            .user_prompt("Hello, how are you?")
            .cwd(PathBuf::from("/tmp"))
            .build();

        let generator = AgentMentionsGenerator;
        let result = generator.generate(&ctx).await.expect("generate");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_agent_mention() {
        let config = test_config();
        let ctx = GeneratorContext::builder()
            .config(&config)
            .turn_number(1)
            .is_main_agent(true)
            .has_user_input(true)
            .user_prompt("Use @agent-search to find the files")
            .cwd(PathBuf::from("/tmp"))
            .build();

        let generator = AgentMentionsGenerator;
        let result = generator.generate(&ctx).await.expect("generate");
        assert!(result.is_some());

        let reminder = result.expect("reminder");
        let content = reminder.content().unwrap();
        assert!(content.contains("invoke the agent"));
        assert!(content.contains("search"));
    }

    #[tokio::test]
    async fn test_multiple_agent_mentions() {
        let config = test_config();
        let ctx = GeneratorContext::builder()
            .config(&config)
            .turn_number(1)
            .is_main_agent(true)
            .has_user_input(true)
            .user_prompt("Use @agent-plan then @agent-edit")
            .cwd(PathBuf::from("/tmp"))
            .build();

        let generator = AgentMentionsGenerator;
        let result = generator.generate(&ctx).await.expect("generate");
        assert!(result.is_some());

        let reminder = result.expect("reminder");
        let content = reminder.content().unwrap();
        assert!(content.contains("invoke the agent \"plan\""));
        assert!(content.contains("invoke the agent \"edit\""));
    }

    #[test]
    fn test_generator_properties() {
        let generator = AgentMentionsGenerator;
        assert_eq!(generator.name(), "AgentMentionsGenerator");
        assert_eq!(generator.tier(), ReminderTier::UserPrompt);
        assert_eq!(generator.attachment_type(), AttachmentType::AgentMentions);
    }
}
