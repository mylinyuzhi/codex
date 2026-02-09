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

/// Key for storing available skills in extension data.
pub const AVAILABLE_SKILLS_KEY: &str = "available_skills";

/// Information about a skill for the system reminder.
#[derive(Debug, Clone)]
pub struct SkillInfo {
    /// Skill name (slash command identifier).
    pub name: String,
    /// Human-readable description.
    pub description: String,
}

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
        // Get skills from extension data
        // The extension builder wraps values in Arc<T>, so the Arc contains Vec<SkillInfo>
        let skills: Option<&Vec<SkillInfo>> = ctx
            .extension_data
            .get(AVAILABLE_SKILLS_KEY)
            .and_then(|v| v.downcast_ref());

        let skills = match skills {
            Some(s) if !s.is_empty() => s,
            _ => return Ok(None),
        };

        let mut content = String::new();
        content.push_str("The following skills are available for use with the Skill tool:\n\n");

        for skill in skills.iter() {
            content.push_str(&format!("- {}: {}\n", skill.name, skill.description));
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
