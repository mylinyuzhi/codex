//! Agent, Skill, SendMessage, TeamCreate, TeamDelete tool implementations.
//!
//! TS: tools/AgentTool/AgentTool.tsx (850+ LOC), tools/shared/spawnMultiAgent.ts
//!
//! The AgentTool dispatches to `ToolUseContext.agent` (AgentHandle trait)
//! to spawn subagents, avoiding circular dependencies between tools and
//! the spawning infrastructure.
//!
//! # Fork subagent (B4.1)
//!
//! The `agent_fork` sibling module implements the fork-subagent support
//! infrastructure in full: `build_fork_context` for byte-identical
//! message cloning, `is_in_fork_child` for recursive-fork prevention,
//! `is_fork_allowed` for the permission gate, and the XML-wrapped
//! rules + FORK_BOILERPLATE_TAG that mark fork contexts. All of this
//! is unit-tested in `agent_fork.test.rs`.
//!
//! **Wiring status**: the AgentTool below does NOT yet invoke the fork
//! path at spawn time. Doing so requires either (a) adding an
//! `Option<ForkContext>` field to `AgentSpawnRequest` in `coco-tool` so
//! it can be threaded to AgentHandle implementations, or (b) having the
//! query-engine layer check `is_fork_allowed()` and construct the fork
//! context itself before calling into AgentTool. Both are cross-crate
//! changes and live in a follow-up commit.
//!
//! For now, callers can opt into fork behavior by setting the
//! `FORK_SUBAGENT=1` env var and omitting `subagent_type` — the
//! AgentHandle implementation at the app/query layer is responsible
//! for detecting this and applying fork semantics.

use std::collections::HashMap;

use coco_tool_runtime::AgentSpawnRequest;
use coco_tool_runtime::AgentSpawnStatus;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolUseContext;
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
    /// TS `AgentTool.tsx`: `isConcurrencySafe() { return true }`. Multiple
    /// agent spawns issued in the same turn are independent — each runs in
    /// its own context (and optionally its own worktree) — so the executor
    /// can batch them into a single `ConcurrentSafe` partition. Without
    /// this override they were forced into per-call `SingleUnsafe` batches,
    /// serializing parallel exploration workflows like
    /// `Agent(...) Agent(...) Agent(...)` and multiplying latency by N.
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
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
                // TS `AgentTool.tsx:99`: ant builds expose `["worktree", "remote"]`,
                // 3p builds expose `["worktree"]` only. coco-rs accepts both values
                // for schema compatibility — the remote path delegates to CCR via
                // the AgentHandle implementation, which can return an error if the
                // current build doesn't support remote isolation.
                "description": "Isolation mode. 'worktree' creates a temporary git worktree so the agent works on an isolated copy of the repo. 'remote' launches the agent in a remote CCR environment (always runs in background).",
                "enum": ["worktree", "remote"]
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

        // Apply TS parent→child permission-mode inheritance rule
        // (runAgent.ts:412-434): trust modes on the parent override the
        // agent's declared mode; otherwise the declaration wins.
        // Isolation-mode early gate. Remote isolation is explicitly
        // unsupported in the current coco-rs build — TS parity: ant
        // builds forward to CCR, but the 3p Rust agent returns a
        // clean model-visible error instead of silently falling back
        // to sync mode. The refactor plan's "Make Unsupported Parity
        // Explicit" rule mandates this rejection shape.
        if let Some("remote") = input.get("isolation").and_then(|v| v.as_str()) {
            return Err(ToolError::ExecutionFailed {
                message: "Isolation mode 'remote' is not supported in this build. \
                          Use 'worktree' for local isolation or omit the field for \
                          no isolation."
                    .into(),
                source: None,
            });
        }

        let requested_mode_str = input.get("mode").and_then(|v| v.as_str()).map(String::from);
        let requested_mode_enum = requested_mode_str.as_deref().and_then(|s| {
            serde_json::from_value::<coco_types::PermissionMode>(serde_json::json!(s)).ok()
        });
        let effective_mode = coco_permissions::resolve_subagent_mode(
            ctx.permission_context.mode,
            requested_mode_enum,
        );
        let effective_mode_str = serde_json::to_value(effective_mode)
            .ok()
            .and_then(|v| v.as_str().map(String::from));

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
                .and_then(serde_json::Value::as_bool)
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
            mode: effective_mode_str,
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
            app_state_patch: None,
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

        // Resolve skill through the dedicated `SkillHandle`. Phase 7
        // of the agent-loop refactor moved skill resolution off
        // `AgentHandle` — skills are a different runtime concept,
        // and the swarm-oriented agent handle was never the right
        // home for expansion + forking.
        let result = ctx
            .skill
            .invoke_skill(skill_name, args)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("Failed to resolve skill '{skill_name}': {e}"),
                source: None,
            })?;

        Ok(ToolResult {
            data: serde_json::json!(result),
            new_messages: vec![],
            app_state_patch: None,
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
            app_state_patch: None,
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
            app_state_patch: None,
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
            app_state_patch: None,
        })
    }
}

#[cfg(test)]
#[path = "agent.test.rs"]
mod tests;
