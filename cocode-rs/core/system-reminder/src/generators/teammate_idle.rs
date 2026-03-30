//! Teammate idle state generator.
//!
//! When a teammate agent has no unread messages and no claimable tasks,
//! this generator injects a reminder with available options: check the
//! task list, ask the lead for work, or wait for messages.
//!
//! Aligned with Claude Code's idle state reminder for team agents.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for teammate idle state reminders.
///
/// Fires when:
/// 1. The agent is part of a team (has team_context)
/// 2. There are no unread messages
/// 3. The agent is not the team leader
///
/// Provides guidance on what an idle teammate should do next.
#[derive(Debug)]
pub struct TeammateIdleGenerator;

#[async_trait]
impl AttachmentGenerator for TeammateIdleGenerator {
    fn name(&self) -> &str {
        "TeammateIdleGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TeammateIdle
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.teammate_idle
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // Show every 3 turns while idle to avoid spamming.
        ThrottleConfig {
            min_turns_between: 3,
            ..Default::default()
        }
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(team) = &ctx.team_context else {
            return Ok(None);
        };

        // Don't show idle reminder if there are unread messages.
        if !ctx.unread_messages.is_empty() {
            return Ok(None);
        }

        // Don't show for the leader (they coordinate, not idle).
        if team.is_leader {
            return Ok(None);
        }

        let display_name = team.agent_name.as_deref().unwrap_or(&team.agent_id);

        let content = IDLE_STATE_INSTRUCTIONS
            .replace("{name}", display_name)
            .replace("{team}", &team.team_name);

        Ok(Some(SystemReminder::new(
            AttachmentType::TeammateIdle,
            content.trim().to_string(),
        )))
    }
}

const IDLE_STATE_INSTRUCTIONS: &str = r#"## Teammate Idle — {name} on team "{team}"

You have no pending messages or active work. Here are your options:

1. **Check the task list** — Use `TaskList` to see if there are unclaimed tasks you can pick up
2. **Ask the lead for work** — Send a message to the team lead via `SendMessage` asking for your next assignment
3. **Send an idle notification** — Use `SendMessage` with `message_type: "idle_notification"` to let the team know you're available
4. **Wait** — If work is being prepared, you'll receive a message when it's ready

Do NOT start implementation work without either claiming a task or receiving instructions from the lead."#;

#[cfg(test)]
#[path = "teammate_idle.test.rs"]
mod tests;
