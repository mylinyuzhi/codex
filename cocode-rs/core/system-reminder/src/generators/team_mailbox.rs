//! Team mailbox generator.
//!
//! Injects unread messages from the agent's mailbox as system reminders.
//! Shutdown requests are formatted prominently to ensure the agent
//! processes them with high priority.

use async_trait::async_trait;

use crate::Result;
use crate::config::SystemReminderConfig;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::generator::message_types;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;

/// Generator for unread mailbox messages.
///
/// Injects pending messages from teammates as system reminders each turn.
/// Shutdown requests are formatted with high-priority headers to ensure
/// the agent handles them promptly.
#[derive(Debug)]
pub struct TeamMailboxGenerator;

#[async_trait]
impl AttachmentGenerator for TeamMailboxGenerator {
    fn name(&self) -> &str {
        "TeamMailboxGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TeamMailbox
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.team_mailbox
    }

    fn throttle_config(&self) -> ThrottleConfig {
        // No throttle — unread messages are urgent
        ThrottleConfig::none()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if ctx.unread_messages.is_empty() {
            return Ok(None);
        }

        let mut shutdown_lines = Vec::new();
        let mut plan_approval_lines = Vec::new();
        let mut regular_lines = Vec::new();

        for msg in &ctx.unread_messages {
            let ts = format_timestamp(msg.timestamp);
            match msg.message_type.as_str() {
                message_types::SHUTDOWN_REQUEST => {
                    shutdown_lines.push(format!(
                        "**From {}** ({ts}): {}\n\
                         Complete your current work, send a summary via SendMessage \
                         with `message_type: \"shutdown_response\"`, then stop.",
                        msg.from, msg.content
                    ));
                }
                message_types::PLAN_APPROVAL_REQUEST => {
                    plan_approval_lines.push(format!(
                        "**From {}** ({ts}): {}\n\
                         Review this plan carefully. To approve, send a message via \
                         SendMessage to \"{}\" with `message_type: \"plan_approval_response\"` \
                         and include `\"approved\": true` in the message. To reject, \
                         include `\"approved\": false` and provide feedback.",
                        msg.from, msg.content, msg.from
                    ));
                }
                message_types::PLAN_APPROVAL_RESPONSE => {
                    plan_approval_lines.push(format!(
                        "**From {}** ({ts}): {}\n\
                         Your plan approval response has been received. If approved, \
                         you may now exit plan mode and begin implementation.",
                        msg.from, msg.content
                    ));
                }
                _ => {
                    regular_lines.push(format!("**From {}** ({ts}): {}", msg.from, msg.content));
                }
            }
        }

        let mut sections = Vec::new();

        if !shutdown_lines.is_empty() {
            sections.push("## SHUTDOWN REQUESTED\n".to_string());
            sections.push("Your team lead has requested that you shut down.\n".to_string());
            for line in &shutdown_lines {
                sections.push(line.clone());
            }
        }

        if !plan_approval_lines.is_empty() {
            sections.push("\n## PLAN APPROVAL\n".to_string());
            for line in &plan_approval_lines {
                sections.push(line.clone());
            }
        }

        if !regular_lines.is_empty() {
            sections.push("\n## Unread Messages\n".to_string());
            for line in &regular_lines {
                sections.push(line.clone());
            }
        }

        Ok(Some(SystemReminder::new(
            AttachmentType::TeamMailbox,
            sections.join("\n").trim().to_string(),
        )))
    }
}

/// Format a Unix timestamp as a human-readable string.
fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt: chrono::DateTime<chrono::Utc>| dt.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| format!("ts:{ts}"))
}

#[cfg(test)]
#[path = "team_mailbox.test.rs"]
mod tests;
