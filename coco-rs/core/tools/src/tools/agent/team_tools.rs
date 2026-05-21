//! `TeamCreateTool` and `TeamDeleteTool` — team lifecycle.
//!
//! TS: `tools/TeamCreateTool/`, `tools/TeamDeleteTool/`. Grouped here
//! because both have a tiny surface and share the `AgentHandle`-routed
//! dispatch shape.

use std::collections::HashMap;

use coco_messages::ToolResult;
use coco_tool_runtime::CreateTeamRequest;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use serde_json::Value;

pub struct TeamCreateTool;

#[async_trait::async_trait]
impl Tool for TeamCreateTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TeamCreate)
    }
    fn name(&self) -> &str {
        ToolName::TeamCreate.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::AgentTeams)
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Create a team of agents for collaborative work.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "team_name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Name for the new team"
            }),
        );
        p.insert(
            "description".into(),
            serde_json::json!({
                "type": "string",
                "description": "Optional description of the team's purpose"
            }),
        );
        p.insert(
            "agent_type".into(),
            serde_json::json!({
                "type": "string",
                "description": "Lead agent type (e.g. 'team-lead', 'researcher')"
            }),
        );
        ToolInputSchema {
            properties: p,
            required: Vec::new(),
        }
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("create a new swarm team with a lead agent")
    }

    /// `data` is the bare confirmation string from `agent.create_team`.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        coco_tool_runtime::render_text_or_json(data)
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let name = input
            .get("team_name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "team_name is required".into(),
                error_code: None,
            });
        }

        let cwd = match ctx.cwd_override.clone() {
            Some(path) => path,
            None => std::env::current_dir().map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to resolve current directory for TeamCreate: {e}"),
                source: None,
            })?,
        };
        let leader_session_id =
            ctx.session_id_for_history
                .clone()
                .ok_or_else(|| ToolError::ExecutionFailed {
                    message: "TeamCreate requires a real leader session id".into(),
                    source: None,
                })?;

        let result = ctx
            .agent
            .create_team(CreateTeamRequest {
                requested_name: name.to_string(),
                leader_agent_id: ctx.agent_id.as_ref().map(ToString::to_string),
                leader_session_id,
                cwd,
                allowed_paths: Vec::new(),
                leader_model: Some(ctx.main_loop_model.clone()),
                task_list_router: ctx.team_task_list_router.clone(),
            })
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: e,
                source: None,
            })?;

        Ok(ToolResult {
            data: serde_json::json!({
                "team_name": result.team_name,
                "lead_agent_id": result.lead_agent_id,
                "task_list_id": result.task_list_id,
            }),
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}

pub struct TeamDeleteTool;

#[async_trait::async_trait]
impl Tool for TeamDeleteTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TeamDelete)
    }
    fn name(&self) -> &str {
        ToolName::TeamDelete.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::AgentTeams)
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Clean up team and task directories when the swarm is complete".into()
    }
    /// TS `TeamDeleteTool.ts:21`: `inputSchema = z.strictObject({})` — the
    /// tool reads the team name from the active session context, not from
    /// tool input. Match the wire shape exactly so callers built against
    /// the TS contract round-trip without per-call adapter logic.
    fn input_schema(&self) -> ToolInputSchema {
        ToolInputSchema {
            properties: HashMap::new(),
            required: Vec::new(),
        }
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("delete the current swarm team and clean up")
    }

    /// Render the prebuilt `message` field — `success` flag is for
    /// callers that key off `data["success"]`.
    fn render_for_model(&self, data: &Value) -> Vec<ToolResultContentPart> {
        let text = data
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| serde_json::to_string(data).unwrap_or_default());
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let result = ctx
            .agent
            .delete_team()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: e,
                source: None,
            })?;

        Ok(ToolResult {
            data: serde_json::json!({ "success": true, "message": result }),
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}
