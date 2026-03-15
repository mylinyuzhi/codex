//! Available skills generator.
//!
//! Injects the list of available skills for the Skill tool.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for available skills reminder.
#[derive(Debug)]
pub struct AvailableSkillsGenerator;

#[async_trait]
impl AttachmentGenerator for AvailableSkillsGenerator {
    fn name(&self) -> &str {
        "AvailableSkillsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AvailableSkills
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::MainAgentOnly
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.available_skills
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Only generate once per session (or every 50 turns as refresh)
        ThrottleConfig {
            min_turns_between: 50,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.available_skills.is_empty() {
            return Ok(None);
        }

        let mut content = String::new();
        content.push_str("The following skills are available for use with the Skill tool:\n\n");

        for skill in &ctx.available_skills {
            content.push_str(&format!("- {}: {}\n", skill.name, skill.description));
            if let Some(ref when) = skill.when_to_use {
                content.push_str(&format!("  When to use: {when}\n"));
            }
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::AvailableSkills,
            content,
        )))
    }
}

#[cfg(test)]
#[path = "available_skills.test.rs"]
mod tests;
