//! TeamDelete tool for removing agent teams.

use super::prompts;
use super::team_state::TeamStore;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;

pub struct TeamDeleteTool {
    store: TeamStore,
}

impl TeamDeleteTool {
    pub fn new(store: TeamStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::TeamDelete.as_str()
    }

    fn description(&self) -> &str {
        prompts::TEAM_DELETE_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the team to delete"
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

        let snapshot = {
            let mut store = self.store.lock().await;
            match store.get(name) {
                None => {
                    return Ok(ToolOutput::error(format!("Team '{name}' not found.")));
                }
                Some(team) => {
                    // Check for active members (excluding the leader)
                    let non_leader_count = team
                        .members
                        .iter()
                        .filter(|m| {
                            team.leader_agent_id
                                .as_ref()
                                .is_none_or(|lid| m.agent_id != *lid)
                        })
                        .count();
                    if non_leader_count > 0 {
                        return Ok(ToolOutput::error(format!(
                            "Team '{name}' has {non_leader_count} active non-leader member(s). \
                             Remove them before deleting the team."
                        )));
                    }
                }
            }
            store.remove(name);
            serde_json::to_value(&*store).unwrap_or_else(|e| {
                tracing::error!("TeamStore serialization failed: {e}");
                serde_json::Value::Object(Default::default())
            })
        };

        ctx.emit_progress(format!("Deleted team '{name}'")).await;

        Ok(
            ToolOutput::text(format!("Team '{name}' deleted successfully."))
                .with_modifier(cocode_protocol::ContextModifier::TeamsUpdated { teams: snapshot }),
        )
    }
}

#[cfg(test)]
#[path = "team_delete.test.rs"]
mod tests;
