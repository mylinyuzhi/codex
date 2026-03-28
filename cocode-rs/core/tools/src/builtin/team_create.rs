//! TeamCreate tool for creating named agent teams.

use std::sync::Arc;

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use cocode_team::Team;
use cocode_team::TeamStore;
use serde_json::Value;

pub struct TeamCreateTool {
    store: Arc<TeamStore>,
}

impl TeamCreateTool {
    pub fn new(store: Arc<TeamStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::TeamCreate.as_str()
    }

    fn description(&self) -> &str {
        prompts::TEAM_CREATE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Unique team name"
                },
                "description": {
                    "type": "string",
                    "description": "Description of the team's purpose"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Default agent type for team members"
                }
            },
            "required": ["name"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Unsafe
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn should_defer(&self) -> bool {
        true
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Collab)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let name = super::input_helpers::require_str(&input, "name")?;

        if !is_valid_team_name(name) {
            return Ok(ToolOutput::error(
                "Invalid team name. Must be 1-64 characters, alphanumeric/hyphen/underscore, \
                 starting with an alphanumeric character."
                    .to_string(),
            ));
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let team = Team {
            name: name.to_string(),
            description: input["description"].as_str().map(String::from),
            agent_type: input["agent_type"].as_str().map(String::from),
            leader_agent_id: ctx.agent_id.clone(),
            members: Vec::new(),
            created_at: now,
        };

        if let Err(e) = self.store.create_team(team).await {
            return Ok(ToolOutput::error(format!("{e}")));
        }

        let snapshot = self.store.snapshot().await;

        ctx.emit_progress(format!("Created team '{name}'")).await;

        Ok(
            ToolOutput::text(format!("Team '{name}' created successfully."))
                .with_modifier(cocode_protocol::ContextModifier::TeamsUpdated { teams: snapshot }),
        )
    }
}

/// Validate team name for filesystem safety and CC alignment.
///
/// Names must be 1-64 chars, start with alphanumeric, contain only
/// alphanumeric, hyphen, or underscore.
fn is_valid_team_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && name.as_bytes()[0].is_ascii_alphanumeric()
}

#[cfg(test)]
#[path = "team_create.test.rs"]
mod tests;
