//! TeamDelete tool for removing agent teams.

use std::sync::Arc;

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use cocode_team::TeamStore;
use serde_json::Value;

pub struct TeamDeleteTool {
    store: Arc<TeamStore>,
}

impl TeamDeleteTool {
    pub fn new(store: Arc<TeamStore>) -> Self {
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
        let name = super::input_helpers::require_str(&input, "name")?;

        // Check for active non-leader members before deletion
        if let Some(team) = self.store.get_team(name).await {
            let active_count = team.active_non_leader_members().len();
            if active_count > 0 {
                return Ok(ToolOutput::error(format!(
                    "Team '{name}' has {active_count} active non-leader member(s). \
                     Remove them before deleting the team."
                )));
            }
        }

        if let Err(e) = self.store.delete_team(name).await {
            return Ok(ToolOutput::error(format!("{e}")));
        }

        let snapshot = self.store.snapshot().await;

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
