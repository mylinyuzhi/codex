//! Invoked skills generator.
//!
//! Injects skill prompt content for skills invoked by the user.
//! This replaces the separate skill injection path with a unified
//! attachment-based system.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for invoked skills.
///
/// Injects skill prompt content when a user invokes a skill via `/skill-name`.
/// The skill content is passed via the typed `invoked_skills` field.
#[derive(Debug)]
pub struct InvokedSkillsGenerator;

#[async_trait]
impl AttachmentGenerator for InvokedSkillsGenerator {
    fn name(&self) -> &str {
        "InvokedSkillsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::InvokedSkills
    }

    fn tier(&self) -> ReminderTier {
        ReminderTier::UserPrompt
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.invoked_skills
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.invoked_skills.is_empty() {
            return Ok(None);
        }

        let mut content = String::new();

        for skill in &ctx.invoked_skills {
            // Format: inject the skill's prompt content with a header
            content.push_str(&format!("<command-name>{}</command-name>\n", skill.name));
            content.push_str(&skill.prompt_content);
            content.push_str("\n\n");
        }

        if content.is_empty() {
            return Ok(None);
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::InvokedSkills,
            content.trim(),
        )))
    }
}

#[cfg(test)]
#[path = "invoked_skills.test.rs"]
mod tests;
