//! Team / swarm reminder generators (3 variants).
//!
//! - [`TeammateMailboxGenerator`] — emits the pre-formatted unread-message
//!   bundle from `formatTeammateMessages` (TS; coco-rs engine/swarm
//!   layer pre-formats and passes the final string via
//!   `ctx.teammate_mailbox`). `attachments.ts:3532`.
//! - [`TeamContextGenerator`] — one-shot "Team Coordination" injection
//!   on the first turn for a teammate. Body matches TS
//!   `messages.ts:3470-3494` verbatim, including the team-leader
//!   paragraph, task-list workflow note, and the JSON message-format
//!   block. The first-turn-only gate is enforced upstream — the engine
//!   passes `Some(snapshot)` only on the first turn — so the generator
//!   itself fires whenever the snapshot is present.
//! - [`AgentPendingMessagesGenerator`] — emits one
//!   `<system-reminder>` per pending teammate message, each wrapped via
//!   `wrapCommandText(coordinator)`. Mirrors TS
//!   `getAgentPendingMessageAttachments` (`attachments.ts:1085-1101`)
//!   which returns `Attachment[]` of N `queued_command` items each
//!   tagged with `origin: { kind: 'coordinator' }`. The wire-level
//!   `AttachmentKind::QueuedCommand` mapping is preserved by
//!   `From<AttachmentType> for AttachmentKind`.
//!
//! All three are TS `allThreadAttachments` (Core tier) and gated on
//! agent-swarms availability upstream; coco-rs leaves that gate to
//! the engine (populates `None` / empty when swarms are disabled).

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::queue_origin::QueueOrigin;
use crate::queue_origin::wrap_command_text;
use crate::types::AttachmentType;
use crate::types::ReminderMessage;
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
        // TS renders the team-coordination prompt with four interpolated
        // fields; any missing field would produce a nonsense injection,
        // so skip if team_name or agent_id is empty.
        if t.team_name.is_empty() || t.agent_id.is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::new(
            AttachmentType::TeamContext,
            render_team_context(t),
        )))
    }
}

/// Render the team-context body. Verbatim from TS `messages.ts:3470-3494`,
/// with the four field substitutions inlined.
///
/// Note: TS does not emit a separate "agent_id" line — only `agentName`,
/// `teamName`, `teamConfigPath`, `taskListPath` are interpolated. The
/// agent_id field on [`crate::TeamContextSnapshot`] is kept for future
/// use (and to gate-check), but is not surfaced in the body.
fn render_team_context(t: &crate::TeamContextSnapshot) -> String {
    format!(
        "# Team Coordination\n\n\
         You are a teammate in team \"{team}\".\n\n\
         **Your Identity:**\n\
         - Name: {name}\n\n\
         **Team Resources:**\n\
         - Team config: {cfg}\n\
         - Task list: {tasks}\n\n\
         **Team Leader:** The team lead's name is \"team-lead\". Send updates and completion notifications to them.\n\n\
         Read the team config to discover your teammates' names. Check the task list periodically. Create new tasks when work should be divided. Mark tasks resolved when complete.\n\n\
         **IMPORTANT:** Always refer to teammates by their NAME (e.g., \"team-lead\", \"analyzer\", \"researcher\"), never by UUID. When messaging, use the name directly:\n\n\
         ```json\n\
         {{\n  \"to\": \"team-lead\",\n  \"message\": \"Your message here\",\n  \"summary\": \"Brief 5-10 word preview\"\n}}\n\
         ```",
        team = t.team_name,
        name = t.agent_name,
        cfg = t.team_config_path,
        tasks = t.task_list_path,
    )
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
        // TS emits one `queued_command` attachment per drained message
        // (`attachments.ts:1095-1100`), each tagged with coordinator
        // origin. We reproduce that shape: N `ReminderMessage`s, each
        // becoming its own `<system-reminder>` block via the inject
        // pipeline (`inject.rs:157-193`). Coordinator framing matches
        // `wrapCommandText` (`messages.ts:5503-5504`).
        //
        // The TS payload is just the message text (`drainPendingMessages`
        // returns `string[]`). The Rust-side `AgentPendingMessage.from`
        // field is intentionally not surfaced in the body to stay
        // byte-equivalent with TS — the coordinator framing already
        // signals the source.
        let messages: Vec<ReminderMessage> = ctx
            .agent_pending_messages
            .iter()
            .filter(|m| !m.text.is_empty())
            .map(|m| {
                ReminderMessage::user_text(wrap_command_text(
                    &m.text,
                    Some(&QueueOrigin::Coordinator),
                ))
            })
            .collect();
        if messages.is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::messages(
            AttachmentType::AgentPendingMessages,
            messages,
        )))
    }
}

#[cfg(test)]
#[path = "team.test.rs"]
mod tests;
