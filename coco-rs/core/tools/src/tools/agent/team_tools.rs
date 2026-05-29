//! `TeamCreateTool` and `TeamDeleteTool` — team lifecycle.
//!
//! TS: `tools/TeamCreateTool/`, `tools/TeamDeleteTool/`. Grouped here
//! because both have a tiny surface and share the `AgentHandle`-routed
//! dispatch shape.

use coco_messages::ToolResult;
use coco_tool_runtime::CreateTeamRequest;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolName;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Typed input for [`TeamCreateTool`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TeamCreateInput {
    /// Name for the new team
    #[serde(default)]
    pub team_name: String,
    /// Optional description of the team's purpose
    #[serde(default)]
    pub description: Option<String>,
    /// Lead agent type (e.g. 'team-lead', 'researcher')
    #[serde(default)]
    pub agent_type: Option<String>,
}

/// Typed output for [`TeamCreateTool`]. All fields default so partial
/// fixtures (`json!({"team_name": "x"})`) round-trip via the blanket.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TeamCreateOutput {
    #[serde(default)]
    pub team_name: String,
    #[serde(default)]
    pub lead_agent_id: String,
    #[serde(default)]
    pub task_list_id: String,
}

pub struct TeamCreateTool;

#[async_trait::async_trait]
impl Tool for TeamCreateTool {
    type Input = TeamCreateInput;
    coco_tool_runtime::impl_runtime_schema!(TeamCreateInput);
    type Output = TeamCreateOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TeamCreate)
    }
    fn name(&self) -> &str {
        ToolName::TeamCreate.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::AgentTeams)
    }
    fn description(&self, _input: &TeamCreateInput, _options: &DescriptionOptions) -> String {
        "Create a team of agents for collaborative work.".into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("create a new swarm team with a lead agent")
    }

    /// Render a compact confirmation. The created `team_name` /
    /// `lead_agent_id` / `task_list_id` are model-visible but the
    /// typical follow-up just needs to know "team is up" — match the
    /// pre-typed default JSON dump shape so consumers can pivot off
    /// `data["team_name"]` etc.
    fn render_for_model(&self, out: &TeamCreateOutput) -> Vec<ToolResultContentPart> {
        let text = serde_json::to_string(out).unwrap_or_default();
        vec![ToolResultContentPart::Text {
            text,
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        input: TeamCreateInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<TeamCreateOutput>, ToolError> {
        if input.team_name.is_empty() {
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
                requested_name: input.team_name.clone(),
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
            data: TeamCreateOutput {
                team_name: result.team_name,
                lead_agent_id: result.lead_agent_id,
                task_list_id: result.task_list_id,
            },
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}

/// Typed input for [`TeamDeleteTool`] — no parameters.
///
/// TS `TeamDeleteTool.ts:21`: `inputSchema = z.strictObject({})` — the
/// tool reads the team name from the active session context, not from
/// tool input.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct TeamDeleteInput {}

/// Typed output for [`TeamDeleteTool`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TeamDeleteOutput {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub message: String,
}

pub struct TeamDeleteTool;

#[async_trait::async_trait]
impl Tool for TeamDeleteTool {
    type Input = TeamDeleteInput;
    coco_tool_runtime::impl_runtime_schema!(TeamDeleteInput);
    type Output = TeamDeleteOutput;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TeamDelete)
    }
    fn name(&self) -> &str {
        ToolName::TeamDelete.as_str()
    }
    fn is_enabled(&self, ctx: &coco_tool_runtime::ToolUseContext) -> bool {
        ctx.features.enabled(coco_types::Feature::AgentTeams)
    }
    fn description(&self, _input: &TeamDeleteInput, _options: &DescriptionOptions) -> String {
        "Clean up team and task directories when the swarm is complete".into()
    }
    fn should_defer(&self) -> bool {
        true
    }
    fn search_hint(&self) -> Option<&str> {
        Some("delete the current swarm team and clean up")
    }

    /// Render the prebuilt `message` field — `success` flag is for
    /// callers that key off `data["success"]`.
    fn render_for_model(&self, out: &TeamDeleteOutput) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: out.message.clone(),
            provider_options: None,
        }]
    }

    async fn execute(
        &self,
        _input: TeamDeleteInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<TeamDeleteOutput>, ToolError> {
        let result = ctx
            .agent
            .delete_team()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: e,
                source: None,
            })?;

        Ok(ToolResult {
            data: TeamDeleteOutput {
                success: true,
                message: result,
            },
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
        })
    }
}
