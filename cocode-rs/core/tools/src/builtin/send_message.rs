//! SendMessage tool for inter-agent communication within teams.
//!
//! Uses an in-memory message store per team (mailbox pattern) instead of
//! agent-resume. Messages are stored and retrieved by recipients via
//! collaboration notification system reminders.

use super::prompts;
use super::team_state::AgentMessage;
use super::team_state::MessageStore;
use super::team_state::TeamStore;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct SendMessageTool {
    team_store: TeamStore,
    message_store: MessageStore,
}

impl SendMessageTool {
    pub fn new(team_store: TeamStore, message_store: MessageStore) -> Self {
        Self {
            team_store,
            message_store,
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
                    "description": "Optional team name to scope the message to"
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
        let to = input["to"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "'to' must be a string",
            }
            .build()
        })?;
        let message = input["message"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "'message' must be a string",
            }
            .build()
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Determine sender ID (use agent_id from context or "main")
        let from = ctx.agent_id.clone().unwrap_or_else(|| "main".to_string());

        // Handle broadcast to all team members
        if to == "all" {
            let team_name = input["team"].as_str().ok_or_else(|| {
                crate::error::tool_error::InvalidInputSnafu {
                    message: "'team' is required when broadcasting to 'all'",
                }
                .build()
            })?;

            let team_store = self.team_store.lock().await;
            let team = team_store.get(team_name).ok_or_else(|| {
                crate::error::tool_error::InvalidInputSnafu {
                    message: format!("Team '{team_name}' not found."),
                }
                .build()
            })?;

            let member_count = team.members.len();
            let mut msg_store = self.message_store.lock().await;
            for member in &team.members {
                if member.agent_id != from {
                    msg_store.push(AgentMessage {
                        from: from.clone(),
                        to: member.agent_id.clone(),
                        content: message.to_string(),
                        timestamp: now,
                        read: false,
                    });
                }
            }

            ctx.emit_progress(format!(
                "Broadcast to {member_count} members of team '{team_name}'"
            ))
            .await;

            return Ok(ToolOutput::text(format!(
                "Message broadcast to {member_count} members of team '{team_name}'."
            )));
        }

        // Validate team membership if team specified
        if let Some(team_name) = input["team"].as_str() {
            let store = self.team_store.lock().await;
            if let Some(team) = store.get(team_name) {
                if !team.has_member(to) {
                    return Ok(ToolOutput::error(format!(
                        "Agent '{to}' is not a member of team '{team_name}'."
                    )));
                }
            } else {
                return Ok(ToolOutput::error(format!("Team '{team_name}' not found.")));
            }
        }

        // Store message in the mailbox
        {
            let mut msg_store = self.message_store.lock().await;
            msg_store.push(AgentMessage {
                from: from.clone(),
                to: to.to_string(),
                content: message.to_string(),
                timestamp: now,
                read: false,
            });
        }

        ctx.emit_progress(format!("Sent message to {to}")).await;

        Ok(ToolOutput::text(format!(
            "Message sent to '{to}' successfully."
        )))
    }
}

#[cfg(test)]
#[path = "send_message.test.rs"]
mod tests;
