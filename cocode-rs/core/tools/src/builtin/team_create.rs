//! TeamCreate tool for creating named agent teams.

use super::prompts;
use super::team_state::Team;
use super::team_state::TeamStore;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct TeamCreateTool {
    store: TeamStore,
}

impl TeamCreateTool {
    pub fn new(store: TeamStore) -> Self {
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

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Collab)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let name = input["name"].as_str().ok_or_else(|| {
            crate::error::tool_error::InvalidInputSnafu {
                message: "name must be a string",
            }
            .build()
        })?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let team = Team {
            name: name.to_string(),
            description: input["description"].as_str().map(String::from),
            agent_type: input["agent_type"].as_str().map(String::from),
            leader_agent_id: None, // Set by the spawning agent context
            members: Vec::new(),
            created_at: now,
        };

        let snapshot = {
            let mut store = self.store.lock().await;
            if store.contains_key(name) {
                return Ok(ToolOutput::error(format!(
                    "Team '{name}' already exists. Delete it first or choose a different name."
                )));
            }
            store.insert(name.to_string(), team);
            serde_json::to_value(&*store).unwrap_or_else(|e| {
                tracing::error!("TeamStore serialization failed: {e}");
                serde_json::Value::Object(Default::default())
            })
        };

        ctx.emit_progress(format!("Created team '{name}'")).await;

        Ok(
            ToolOutput::text(format!("Team '{name}' created successfully."))
                .with_modifier(cocode_protocol::ContextModifier::TeamsUpdated { teams: snapshot }),
        )
    }
}

#[cfg(test)]
#[path = "team_create.test.rs"]
mod tests;
