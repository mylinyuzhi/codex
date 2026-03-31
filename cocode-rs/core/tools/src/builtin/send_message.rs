//! SendMessage tool for inter-agent communication within teams.
//!
//! Uses a filesystem-backed mailbox per agent (JSONL files with atomic writes)
//! for persistent inter-agent communication. Supports message types including
//! shutdown requests, responses, idle notifications, and broadcast.

use std::sync::Arc;

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use cocode_team::AgentMessage;
use cocode_team::Mailbox;
use cocode_team::MemberStatus;
use cocode_team::MessageType;
use cocode_team::TeamStore;
use serde_json::Value;

pub struct SendMessageTool {
    team_store: Arc<TeamStore>,
    mailbox: Arc<Mailbox>,
}

impl SendMessageTool {
    pub fn new(team_store: Arc<TeamStore>, mailbox: Arc<Mailbox>) -> Self {
        Self {
            team_store,
            mailbox,
        }
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::SendMessage.as_str()
    }

    fn description(&self) -> &str {
        prompts::SEND_MESSAGE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Agent ID or name to send the message to. Use \"all\" to broadcast to all team members."
                },
                "message": {
                    "type": "string",
                    "description": "The message content to send"
                },
                "team": {
                    "type": "string",
                    "description": "Team name to scope the message to (required for direct messages and broadcast)"
                },
                "message_type": {
                    "type": "string",
                    "enum": [
                        MessageType::Message.as_str(),
                        MessageType::Broadcast.as_str(),
                        MessageType::ShutdownRequest.as_str(),
                        MessageType::ShutdownResponse.as_str(),
                        MessageType::PlanApprovalRequest.as_str(),
                        MessageType::PlanApprovalResponse.as_str(),
                        MessageType::IdleNotification.as_str(),
                    ],
                    "description": "Type of message. Defaults to 'message'."
                }
            },
            "required": ["to", "message"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Collab)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let to = super::input_helpers::require_str(&input, "to")?;
        let message = super::input_helpers::require_str(&input, "message")?;

        let message_type = match input["message_type"].as_str() {
            Some(s) if s == MessageType::Broadcast.as_str() => MessageType::Broadcast,
            Some(s) if s == MessageType::ShutdownRequest.as_str() => MessageType::ShutdownRequest,
            Some(s) if s == MessageType::ShutdownResponse.as_str() => MessageType::ShutdownResponse,
            Some(s) if s == MessageType::PlanApprovalRequest.as_str() => {
                MessageType::PlanApprovalRequest
            }
            Some(s) if s == MessageType::PlanApprovalResponse.as_str() => {
                MessageType::PlanApprovalResponse
            }
            Some(s) if s == MessageType::IdleNotification.as_str() => MessageType::IdleNotification,
            _ => MessageType::Message,
        };

        // Determine sender ID
        let from = ctx
            .identity
            .agent_id
            .clone()
            .unwrap_or_else(|| "main".to_string());
        let team_name = input["team"].as_str();

        // Require team for all message types (no magic default)
        let effective_team = match team_name {
            Some(tn) => tn,
            None => {
                return Ok(ToolOutput::error(
                    "'team' is required. Specify the team name to scope this message.".to_string(),
                ));
            }
        };

        // Build the message
        let msg = AgentMessage::new(&from, to, message, message_type).with_team(effective_team);

        // Handle broadcast to all team members
        if to == "all" || message_type == MessageType::Broadcast {
            let team = self
                .team_store
                .get_team(effective_team)
                .await
                .ok_or_else(|| {
                    crate::error::tool_error::InvalidInputSnafu {
                        message: format!("Team '{effective_team}' not found."),
                    }
                    .build()
                })?;

            let member_ids = team.agent_ids();
            if let Err(e) = self
                .mailbox
                .broadcast(effective_team, &msg, &member_ids)
                .await
            {
                return Ok(ToolOutput::error(format!("Broadcast failed: {e}")));
            }

            ctx.emit_progress(format!(
                "Broadcast to {} members of team '{effective_team}'",
                member_ids.len()
            ))
            .await;

            return Ok(ToolOutput::text(format!(
                "Message broadcast to {} members of team '{effective_team}'.",
                member_ids.len()
            )));
        }

        // Validate team membership
        match self.team_store.get_team(effective_team).await {
            Some(team) => {
                if !team.has_member(to) {
                    return Ok(ToolOutput::error(format!(
                        "Agent '{to}' is not a member of team '{effective_team}'."
                    )));
                }
            }
            None => {
                return Ok(ToolOutput::error(format!(
                    "Team '{effective_team}' not found."
                )));
            }
        }

        // Send the message via mailbox
        if let Err(e) = self.mailbox.send(effective_team, &msg).await {
            return Ok(ToolOutput::error(format!("Send failed: {e}")));
        }

        // Shutdown side effects: update member status in team store
        match message_type {
            MessageType::ShutdownRequest => {
                let _ = self
                    .team_store
                    .update_member_status(effective_team, to, MemberStatus::ShuttingDown)
                    .await;
            }
            MessageType::ShutdownResponse => {
                let _ = self
                    .team_store
                    .update_member_status(effective_team, &from, MemberStatus::Stopped)
                    .await;
            }
            _ => {}
        }

        ctx.emit_progress(format!("Sent {message_type} to {to}"))
            .await;

        Ok(ToolOutput::text(format!(
            "Message sent to '{to}' successfully."
        )))
    }
}

#[cfg(test)]
#[path = "send_message.test.rs"]
mod tests;
