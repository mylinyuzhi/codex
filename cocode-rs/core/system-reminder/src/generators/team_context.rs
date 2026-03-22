//! Team context generator.
//!
//! Injects team identity, member list, and communication instructions
//! for agents that are part of a team. Runs every turn (no throttle)
//! so teammates always know who they are and who else is on the team.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for team context injection.
///
/// When the current agent is part of a team, this generator produces a
/// system reminder containing the agent's identity, the team member list,
/// and communication instructions. This ensures teammates always have
/// up-to-date awareness of the team structure.
#[derive(Debug)]
pub struct TeamContextGenerator;

#[async_trait]
impl AttachmentGenerator for TeamContextGenerator {
    fn name(&self) -> &str {
        "TeamContextGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TeamContext
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.team_context
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle — teammates always need to know who they are
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(team) = &ctx.team_context else {
            return Ok(None);
        };

        let mut lines = Vec::new();

        // Agent identity
        let display_name = team.agent_name.as_deref().unwrap_or(&team.agent_id);
        lines.push(format!(
            "## Team Context\n\nYou are **{display_name}** (`{}`), a member of team \"**{}**\".",
            team.agent_id, team.team_name
        ));
        lines.push(format!("Your role: {}", team.agent_type));

        // Team members
        if !team.members.is_empty() {
            lines.push("\n### Team Members\n".to_string());
            for member in &team.members {
                let name = member.name.as_deref().unwrap_or(&member.agent_id);
                let agent_type = member.agent_type.as_deref().unwrap_or("unknown");
                lines.push(format!(
                    "- **{name}** (`{}`) — {agent_type} — {}",
                    member.agent_id, member.status
                ));
            }
        }

        // Communication instructions
        lines.push("\n### Communication\n".to_string());
        lines.push(
            "- Use `SendMessage` to communicate with teammates.\n\
             - Set `to` to a teammate's name or ID, or `\"all\"` to broadcast.\n\
             - Set `message_type` to `\"shutdown_request\"` to request graceful shutdown."
                .to_string(),
        );

        Ok(Some(SystemReminder::new(
            AttachmentType::TeamContext,
            lines.join("\n").trim().to_string(),
        )))
    }
}

#[cfg(test)]
#[path = "team_context.test.rs"]
mod tests;
