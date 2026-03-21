//! Available skills generator.
//!
//! Injects the list of available skills for the Skill tool.
//! Uses delta tracking (only sends new skills) and budget-aware formatting
//! with 3-tier truncation aligned with Claude Code v2.1.76.

use std::collections::HashSet;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::SkillInfo;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::ReminderTier;
use crate::types::SystemReminder;

/// Generator for available skills reminder.
///
/// Stateful: tracks which skill names have already been sent so only
/// new skills are included in subsequent reminders.
#[derive(Debug)]
pub struct AvailableSkillsGenerator {
    /// Names of skills already sent to the model.
    sent_skill_names: Mutex<HashSet<String>>,
    /// Whether the initial call has been made. On the first call we populate
    /// `sent_skill_names` but return `None` because the skills are already
    /// present in the system prompt tool definitions.
    is_initial: Mutex<bool>,
}

impl AvailableSkillsGenerator {
    /// Create a new generator.
    pub fn new() -> Self {
        Self {
            sent_skill_names: Mutex::new(HashSet::new()),
            is_initial: Mutex::new(true),
        }
    }
}

impl Default for AvailableSkillsGenerator {
    fn default() -> Self {
        Self::new()
    }
}

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
        // Delta tracking handles frequency — allow every turn
        ThrottleConfig {
            min_turns_between: 1,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.available_skills.is_empty() {
            return Ok(None);
        }

        // On the first call, record all current skill names but don't emit
        // (they are already in the system prompt via tool definitions).
        {
            let mut is_initial = self
                .is_initial
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if *is_initial {
                *is_initial = false;
                let mut sent = self
                    .sent_skill_names
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for skill in &ctx.available_skills {
                    sent.insert(skill.name.clone());
                }
                return Ok(None);
            }
        }

        // Compute delta: skills not yet sent
        let mut sent = self
            .sent_skill_names
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let new_skills: Vec<&SkillInfo> = ctx
            .available_skills
            .iter()
            .filter(|s| !sent.contains(&s.name))
            .collect();

        if new_skills.is_empty() {
            return Ok(None);
        }

        // Record these as sent
        for skill in &new_skills {
            sent.insert(skill.name.clone());
        }
        drop(sent);

        // Format within budget
        let content = format_skills_within_budget(&new_skills, ctx.context_window);

        Ok(Some(SystemReminder::new(
            AttachmentType::AvailableSkills,
            content,
        )))
    }
}

/// Format skills within a character budget using 3-tier truncation.
///
/// Budget calculation: `min(16000, context_window_tokens * 4 * 2 / 100)`
///
/// - **Tier 1**: Full entries (name + description + when_to_use). Used if total fits.
/// - **Tier 2**: Bundled skills always full. Non-bundled descriptions truncated to
///   per-skill budget.
/// - **Tier 3**: Bundled skills full. Non-bundled skills shown as names only.
fn format_skills_within_budget(skills: &[&SkillInfo], context_window_tokens: i32) -> String {
    let char_budget = std::cmp::min(16000, (context_window_tokens as i64) * 4 * 2 / 100) as usize;

    // Tier 1: try full format
    let full = format_skills_full(skills);
    if full.len() <= char_budget {
        return full;
    }

    // Tier 2: truncate non-bundled descriptions
    let non_bundled_count = skills.iter().filter(|s| !s.is_bundled).count();
    let bundled_size: usize = skills
        .iter()
        .filter(|s| s.is_bundled)
        .map(|s| format_single_skill_full(s).len())
        .sum();
    let remaining = char_budget.saturating_sub(bundled_size).saturating_sub(80); // header
    let per_skill_budget = if non_bundled_count > 0 {
        remaining / non_bundled_count
    } else {
        0
    };

    let truncated = format_skills_truncated(skills, per_skill_budget);
    if truncated.len() <= char_budget {
        return truncated;
    }

    // Tier 3: bundled full, non-bundled names only
    format_skills_names_only(skills)
}

fn format_skills_full(skills: &[&SkillInfo]) -> String {
    let mut content = String::new();
    content.push_str("The following skills are available for use with the Skill tool:\n\n");
    for skill in skills {
        content.push_str(&format_single_skill_full(skill));
    }
    content
}

fn format_single_skill_full(skill: &SkillInfo) -> String {
    let plugin_attr = skill
        .plugin_name
        .as_ref()
        .map(|p| format!(" (from {p})"))
        .unwrap_or_default();
    let mut entry = format!("- {}{plugin_attr}: {}\n", skill.name, skill.description);
    if let Some(ref when) = skill.when_to_use {
        entry.push_str(&format!("  When to use: {when}\n"));
    }
    entry
}

fn format_skills_truncated(skills: &[&SkillInfo], per_skill_budget: usize) -> String {
    let mut content = String::new();
    content.push_str("The following skills are available for use with the Skill tool:\n\n");
    for skill in skills {
        if skill.is_bundled {
            content.push_str(&format_single_skill_full(skill));
        } else {
            let plugin_attr = skill
                .plugin_name
                .as_ref()
                .map(|p| format!(" (from {p})"))
                .unwrap_or_default();
            let mut desc = skill.description.clone();
            if per_skill_budget > 10 && desc.len() > per_skill_budget {
                desc.truncate(per_skill_budget.saturating_sub(3));
                desc.push_str("...");
            }
            content.push_str(&format!("- {}{plugin_attr}: {desc}\n", skill.name));
        }
    }
    content
}

fn format_skills_names_only(skills: &[&SkillInfo]) -> String {
    let mut content = String::new();
    content.push_str("The following skills are available for use with the Skill tool:\n\n");
    for skill in skills {
        if skill.is_bundled {
            content.push_str(&format_single_skill_full(skill));
        } else {
            let plugin_attr = skill
                .plugin_name
                .as_ref()
                .map(|p| format!(" (from {p})"))
                .unwrap_or_default();
            content.push_str(&format!("- {}{plugin_attr}\n", skill.name));
        }
    }
    content
}

#[cfg(test)]
#[path = "available_skills.test.rs"]
mod tests;
