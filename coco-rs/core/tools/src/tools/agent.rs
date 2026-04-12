//! Agent, Skill, SendMessage, TeamCreate, TeamDelete tool implementations.
//!
//! TS: tools/AgentTool/AgentTool.tsx (850+ LOC), tools/shared/spawnMultiAgent.ts
//!
//! The AgentTool dispatches to `ToolUseContext.agent` (AgentHandle trait)
//! to spawn subagents, avoiding circular dependencies between tools and
//! the spawning infrastructure.

use std::collections::HashMap;

use coco_tool::AgentSpawnRequest;
use coco_tool::AgentSpawnStatus;
use coco_tool::DescriptionOptions;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;

// ── AgentTool ──

/// Launch a specialized agent for complex, multi-step tasks.
///
/// TS: tools/AgentTool/AgentTool.tsx
pub struct AgentTool;

#[async_trait::async_trait]
impl Tool for AgentTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Agent)
    }
    fn name(&self) -> &str {
        ToolName::Agent.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Launch a new agent to handle complex, multi-step tasks autonomously.\n\n\
         The Agent tool launches specialized agents (subprocesses) that \
         autonomously handle complex tasks. Each agent type has specific \
         capabilities and tools available to it."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "prompt".into(),
            serde_json::json!({
                "type": "string",
                "description": "The task for the agent to perform"
            }),
        );
        p.insert(
            "description".into(),
            serde_json::json!({
                "type": "string",
                "description": "A short (3-5 word) description of the task"
            }),
        );
        p.insert(
            "subagent_type".into(),
            serde_json::json!({
                "type": "string",
                "description": "The type of specialized agent to use (e.g., 'Explore', 'Plan', 'general-purpose')"
            }),
        );
        p.insert(
            "model".into(),
            serde_json::json!({
                "type": "string",
                "description": "Optional model override for this agent",
                "enum": ["sonnet", "opus", "haiku"]
            }),
        );
        p.insert(
            "run_in_background".into(),
            serde_json::json!({
                "type": "boolean",
                "description": "Set to true to run this agent in the background"
            }),
        );
        p.insert(
            "isolation".into(),
            serde_json::json!({
                "type": "string",
                "description": "Isolation mode: 'worktree' creates an isolated git worktree",
                "enum": ["worktree"]
            }),
        );
        p.insert(
            "name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Agent name for multi-agent teams (used with team_name)"
            }),
        );
        p.insert(
            "team_name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Team name for spawning a teammate"
            }),
        );
        p.insert(
            "mode".into(),
            serde_json::json!({
                "type": "string",
                "description": "Permission mode override (e.g., 'plan')"
            }),
        );
        p.insert(
            "cwd".into(),
            serde_json::json!({
                "type": "string",
                "description": "Working directory override for the agent"
            }),
        );
        ToolInputSchema { properties: p }
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if prompt.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "prompt is required and must be non-empty".into(),
                error_code: None,
            });
        }

        let request = AgentSpawnRequest {
            prompt: prompt.to_string(),
            description: input
                .get("description")
                .and_then(|v| v.as_str())
                .map(String::from),
            subagent_type: input
                .get("subagent_type")
                .and_then(|v| v.as_str())
                .map(String::from),
            model: input
                .get("model")
                .and_then(|v| v.as_str())
                .map(String::from),
            run_in_background: input
                .get("run_in_background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            isolation: input
                .get("isolation")
                .and_then(|v| v.as_str())
                .map(String::from),
            name: input.get("name").and_then(|v| v.as_str()).map(String::from),
            team_name: input
                .get("team_name")
                .and_then(|v| v.as_str())
                .map(String::from),
            mode: input.get("mode").and_then(|v| v.as_str()).map(String::from),
            cwd: input
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from),
        };

        let request_description = request.description.clone();

        let response =
            ctx.agent
                .spawn_agent(request)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: e,
                    source: None,
                })?;

        let data = match response.status {
            AgentSpawnStatus::Completed => {
                let mut result = serde_json::json!({
                    "status": "completed",
                    "content": response.result.unwrap_or_default(),
                    "prompt": response.prompt.as_deref().unwrap_or(prompt),
                    "totalToolUseCount": response.total_tool_use_count,
                    "totalTokens": response.total_tokens,
                    "durationMs": response.duration_ms,
                });
                if let Some(wt) = &response.worktree_path {
                    result["worktreePath"] = serde_json::json!(wt);
                }
                if let Some(wb) = &response.worktree_branch {
                    result["worktreeBranch"] = serde_json::json!(wb);
                }
                result
            }
            AgentSpawnStatus::AsyncLaunched => {
                let mut result = serde_json::json!({
                    "status": "async_launched",
                    "agentId": response.agent_id,
                    "prompt": response.prompt.as_deref().unwrap_or(prompt),
                    "description": request_description,
                });
                if let Some(of) = &response.output_file {
                    result["outputFile"] = serde_json::json!(of);
                }
                result
            }
            AgentSpawnStatus::TeammateSpawned => {
                serde_json::json!({
                    "status": "teammate_spawned",
                    "agentId": response.agent_id,
                    "prompt": response.prompt.as_deref().unwrap_or(prompt),
                })
            }
            AgentSpawnStatus::Failed => {
                serde_json::json!({
                    "status": "failed",
                    "error": response.error.unwrap_or_else(|| "Unknown error".into()),
                })
            }
        };

        Ok(ToolResult {
            data,
            new_messages: vec![],
        })
    }
}

// ── SkillTool ──

/// Execute a skill (slash command) within the main conversation.
///
/// TS: tools/SkillTool/
pub struct SkillTool;

#[async_trait::async_trait]
impl Tool for SkillTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Skill)
    }
    fn name(&self) -> &str {
        ToolName::Skill.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Execute a skill within the main conversation. Skills provide specialized \
         capabilities and domain knowledge."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "skill".into(),
            serde_json::json!({
                "type": "string",
                "description": "The skill name to invoke (e.g. 'commit', 'review-pr', 'pdf')"
            }),
        );
        p.insert(
            "args".into(),
            serde_json::json!({
                "type": "string",
                "description": "Optional arguments for the skill"
            }),
        );
        ToolInputSchema { properties: p }
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let skill_name = input
            .get("skill")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if skill_name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "skill name is required".into(),
                error_code: None,
            });
        }

        let args = input
            .get("args")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        // Resolve skill via the agent handle, which delegates to the
        // app-level skill manager.  Returns the skill definition for the
        // query engine to expand inline or fork.
        let result = ctx
            .agent
            .resolve_skill(skill_name, args)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to resolve skill '{skill_name}': {e}"),
                source: None,
            })?;

        Ok(ToolResult {
            data: serde_json::json!(result),
            new_messages: vec![],
        })
    }
}

// ── SendMessageTool ──

/// Send a message to a teammate or broadcast to the team.
///
/// TS: tools/SendMessageTool/
pub struct SendMessageTool;

#[async_trait::async_trait]
impl Tool for SendMessageTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::SendMessage)
    }
    fn name(&self) -> &str {
        ToolName::SendMessage.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Send a message to another agent in the team. Use the agent's name \
         as target, or \"*\" to broadcast to all teammates."
            .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "to".into(),
            serde_json::json!({
                "type": "string",
                "description": "Target agent name, \"*\" for broadcast, or agent ID"
            }),
        );
        p.insert(
            "summary".into(),
            serde_json::json!({
                "type": "string",
                "description": "Brief summary of the message (5-10 words)"
            }),
        );
        p.insert(
            "message".into(),
            serde_json::json!({
                "description": "Message content (string or structured object)",
                "oneOf": [
                    {"type": "string"},
                    {"type": "object", "properties": {
                        "type": {"type": "string", "enum": [
                            "shutdown_request", "shutdown_response", "plan_approval_response"
                        ]},
                        "request_id": {"type": "string"},
                        "approve": {"type": "boolean"},
                        "reason": {"type": "string"},
                        "feedback": {"type": "string"}
                    }}
                ]
            }),
        );
        ToolInputSchema { properties: p }
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let to = input.get("to").and_then(|v| v.as_str()).unwrap_or_default();

        if to.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "target agent name or ID ('to') is required".into(),
                error_code: None,
            });
        }

        // Extract message content — string or structured JSON
        let content = if let Some(msg) = input.get("message") {
            if let Some(s) = msg.as_str() {
                s.to_string()
            } else {
                // Structured message — serialize to JSON for mailbox
                serde_json::to_string(msg).unwrap_or_default()
            }
        } else {
            return Err(ToolError::InvalidInput {
                message: "message content is required".into(),
                error_code: None,
            });
        };

        if content.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "message content must be non-empty".into(),
                error_code: None,
            });
        }

        let result =
            ctx.agent
                .send_message(to, &content)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    message: e,
                    source: None,
                })?;

        Ok(ToolResult {
            data: serde_json::json!(result),
            new_messages: vec![],
        })
    }
}

// ── TeamCreateTool ──

/// Create a team of agents for collaborative work.
///
/// TS: tools/TeamCreateTool/
pub struct TeamCreateTool;

#[async_trait::async_trait]
impl Tool for TeamCreateTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TeamCreate)
    }
    fn name(&self) -> &str {
        ToolName::TeamCreate.as_str()
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
        ToolInputSchema { properties: p }
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

        let result = ctx
            .agent
            .create_team(name)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: e,
                source: None,
            })?;

        Ok(ToolResult {
            data: serde_json::json!(result),
            new_messages: vec![],
        })
    }
}

// ── TeamDeleteTool ──

/// Delete a team and release its resources.
///
/// TS: tools/TeamDeleteTool/
pub struct TeamDeleteTool;

#[async_trait::async_trait]
impl Tool for TeamDeleteTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::TeamDelete)
    }
    fn name(&self) -> &str {
        ToolName::TeamDelete.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        "Delete a team and release its resources.".into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "name".into(),
            serde_json::json!({
                "type": "string",
                "description": "Name of the team to delete"
            }),
        );
        ToolInputSchema { properties: p }
    }
    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if name.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "team name is required".into(),
                error_code: None,
            });
        }

        let result = ctx
            .agent
            .delete_team(name)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: e,
                source: None,
            })?;

        Ok(ToolResult {
            data: serde_json::json!(result),
            new_messages: vec![],
        })
    }
}

#[cfg(test)]
#[path = "agent.test.rs"]
mod tests;
