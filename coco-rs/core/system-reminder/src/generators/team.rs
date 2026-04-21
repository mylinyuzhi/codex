//! Team / swarm reminder generators (3 variants).
//!
//! - `TeammateMailboxGenerator` — emits pre-formatted unread-message
//!   bundle from `formatTeammateMessages` (TS; coco-rs engine/swarm
//!   layer pre-formats and passes the final string via
//!   `ctx.teammate_mailbox`). `attachments.ts:3532`.
//! - `TeamContextGenerator` — one-shot "Team Coordination" injection
//!   on the first turn for a teammate. `messages.ts:3795-3804`.
//! - `AgentPendingMessagesGenerator` — lists queued inbox messages for
//!   the agent. `attachments.ts:916`.
//!
//! All three are TS `allThreadAttachments` (Core tier) and gated on
//! agent-swarms availability upstream; coco-rs leaves that gate to
//! the engine (populates `None` / empty when swarms are disabled).

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

// ---------------------------------------------------------------------------
// TeammateMailboxGenerator
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct TeammateMailboxGenerator;

#[async_trait]
impl AttachmentGenerator for TeammateMailboxGenerator {
    fn name(&self) -> &str {
        "TeammateMailboxGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TeammateMailbox
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.teammate_mailbox
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(info) = ctx.teammate_mailbox.as_ref() else {
            return Ok(None);
        };
        if info.formatted.is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::new(
            AttachmentType::TeammateMailbox,
            info.formatted.clone(),
        )))
    }
}

// ---------------------------------------------------------------------------
// TeamContextGenerator
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
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

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(t) = ctx.team_context.as_ref() else {
            return Ok(None);
        };
        // TS renders the team-coordination prompt with four fields;
        // any missing field would produce a nonsense injection, so
        // skip if team_name or agent_id is empty.
        if t.team_name.is_empty() || t.agent_id.is_empty() {
            return Ok(None);
        }
        let body = format!(
            "# Team Coordination\n\nYou are a teammate in team \"{team}\".\nYour agent id: {agent_id}\nYour display name: {agent_name}\nTeam config path: {cfg}\nShared task list path: {tasks}",
            team = t.team_name,
            agent_id = t.agent_id,
            agent_name = t.agent_name,
            cfg = t.team_config_path,
            tasks = t.task_list_path,
        );
        Ok(Some(SystemReminder::new(AttachmentType::TeamContext, body)))
    }
}

// ---------------------------------------------------------------------------
// AgentPendingMessagesGenerator
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct AgentPendingMessagesGenerator;

#[async_trait]
impl AttachmentGenerator for AgentPendingMessagesGenerator {
    fn name(&self) -> &str {
        "AgentPendingMessagesGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AgentPendingMessages
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.agent_pending_messages
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.agent_pending_messages.is_empty() {
            return Ok(None);
        }
        let lines: Vec<String> = ctx
            .agent_pending_messages
            .iter()
            .map(|m| format!("- from {from}: {text}", from = m.from, text = m.text))
            .collect();
        let body = format!(
            "You have pending messages from teammates:\n{}",
            lines.join("\n")
        );
        Ok(Some(SystemReminder::new(
            AttachmentType::AgentPendingMessages,
            body,
        )))
    }
}

#[cfg(test)]
#[path = "team.test.rs"]
mod tests;
