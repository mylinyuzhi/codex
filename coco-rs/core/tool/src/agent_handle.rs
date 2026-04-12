//! Agent handle trait — async agent operations abstraction for tools.
//!
//! TS: tools/AgentTool/AgentTool.tsx (spawn), tools/shared/spawnMultiAgent.ts (team)
//!
//! **Split design** (same pattern as SideQuery):
//! - Async trait (`AgentHandle`) -> here in `coco-tool`
//! - Implementations -> app/state or executor layer
//! - Tools access via `ToolUseContext.agent`
//!
//! **Dependency flow**:
//! ```text
//! coco-types         (AgentDefinition, AgentIsolation, SubagentType)
//!     |
//! coco-tool          (defines async AgentHandle trait, puts Arc<dyn> on ToolUseContext)
//!     |
//! coco-tools         (AgentTool/SendMessageTool/TeamCreate/TeamDelete call handle methods)
//!     |
//! coco-state         (implements AgentHandle using swarm infrastructure)
//!     |
//! coco-executor      (wires implementation into ToolUseContext)
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

/// Request to spawn a subagent.
///
/// TS: AgentToolInput in AgentTool.tsx
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpawnRequest {
    /// The task/instruction for the agent.
    pub prompt: String,
    /// Short (3-5 word) description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Agent type to use (e.g., "Explore", "Plan", "general-purpose").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent_type: Option<String>,
    /// Model override (e.g., "sonnet", "opus", "haiku").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Run in background (fire-and-forget).
    #[serde(default)]
    pub run_in_background: bool,
    /// Isolation mode ("worktree" or "remote").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    /// Agent name (for multi-agent teams).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Team name (triggers teammate spawn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    /// Permission mode override (e.g., "plan").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Working directory override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

/// Response from spawning a subagent.
///
/// TS: AgentTool call result variants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpawnResponse {
    /// Outcome of the spawn.
    pub status: AgentSpawnStatus,
    /// Agent identifier (for async/team agents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Result text (for completed sync agents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Error message (for failed spawns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Total tool uses by the agent.
    #[serde(default)]
    pub total_tool_use_count: i64,
    /// Total tokens consumed.
    #[serde(default)]
    pub total_tokens: i64,
    /// Duration in milliseconds.
    #[serde(default)]
    pub duration_ms: i64,
    /// Worktree path (if isolation was used).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<PathBuf>,
    /// Worktree branch name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    /// Output file path for background agents.
    /// TS: getTaskOutputPath(agentId) — returned in async_launched responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_file: Option<PathBuf>,
    /// The original prompt (echoed back in response).
    /// TS: AgentTool output includes prompt field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

/// Outcome of a spawn request.
///
/// TS: AgentTool return status variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSpawnStatus {
    /// Synchronous agent completed successfully.
    Completed,
    /// Background agent launched (poll for result).
    AsyncLaunched,
    /// Teammate spawned in a team.
    TeammateSpawned,
    /// Agent spawn failed.
    Failed,
}

/// Trait for agent operations from tools.
///
/// Implementations live in the app/state or executor layer. Tools access
/// this via `ToolUseContext.agent`.
#[async_trait::async_trait]
pub trait AgentHandle: Send + Sync {
    /// Spawn a subagent (sync or async).
    ///
    /// TS: AgentTool.call() in AgentTool.tsx
    async fn spawn_agent(&self, request: AgentSpawnRequest) -> Result<AgentSpawnResponse, String>;

    /// Send a message to another agent by name or ID.
    /// Use `"*"` as target to broadcast to all teammates.
    ///
    /// Content may be a plain text string or a serialized structured
    /// message (shutdown_request, shutdown_response, plan_approval_response).
    ///
    /// TS: SendMessageTool routing via agent_name_registry
    async fn send_message(&self, to: &str, content: &str) -> Result<String, String>;

    /// Create a new team with optional description and lead agent type.
    ///
    /// TS: TeamCreateTool → TeamFile creation + AppState update
    async fn create_team(&self, name: &str) -> Result<String, String>;

    /// Delete a team and release resources.
    /// Fails if non-lead members are still active.
    ///
    /// TS: TeamDeleteTool → cleanup + AppState clear
    async fn delete_team(&self, name: &str) -> Result<String, String>;

    /// Resume a previously interrupted agent.
    ///
    /// TS: resumeAgentBackground() in resumeAgent.ts
    async fn resume_agent(
        &self,
        agent_id: &str,
        prompt: Option<&str>,
    ) -> Result<AgentSpawnResponse, String>;

    /// Query the status of a background agent.
    ///
    /// Returns the agent's current status and result if completed.
    /// TS: checkAgentStatus() in LocalAgentTask
    async fn query_agent_status(&self, agent_id: &str) -> Result<AgentSpawnResponse, String>;

    /// Get the output of a completed background agent.
    ///
    /// TS: getAgentOutput() — reads the output file from a completed agent.
    async fn get_agent_output(&self, agent_id: &str) -> Result<String, String>;

    /// Signal that a foreground agent should move to background execution.
    ///
    /// The agent continues running but unblocks the parent turn.
    /// TS: backgroundSignal + wasBackgrounded logic in AgentTool.tsx
    async fn background_agent(&self, agent_id: &str) -> Result<(), String>;

    /// Resolve a skill by name and return its expanded content.
    ///
    /// Returns a JSON value with `skill_name`, `context` (inline/fork),
    /// `prompt` (expanded content), `allowed_tools`, and optional `model`
    /// override. The query engine uses this to either expand the skill
    /// inline or fork a sub-agent.
    ///
    /// TS: SkillTool.call() → getCommands() → findCommand() → getPromptForCommand()
    async fn resolve_skill(&self, name: &str, args: &str) -> Result<serde_json::Value, String>;
}

/// Shared handle type for `ToolUseContext`.
pub type AgentHandleRef = Arc<dyn AgentHandle>;

/// A no-op implementation that returns errors. Used in test/stub contexts.
#[derive(Debug, Clone)]
pub struct NoOpAgentHandle;

#[async_trait::async_trait]
impl AgentHandle for NoOpAgentHandle {
    async fn spawn_agent(&self, _request: AgentSpawnRequest) -> Result<AgentSpawnResponse, String> {
        Err("Agent spawning not available in this context".into())
    }

    async fn send_message(&self, _to: &str, _content: &str) -> Result<String, String> {
        Err("Agent messaging not available in this context".into())
    }

    async fn create_team(&self, _name: &str) -> Result<String, String> {
        Err("Team management not available in this context".into())
    }

    async fn delete_team(&self, _name: &str) -> Result<String, String> {
        Err("Team management not available in this context".into())
    }

    async fn resume_agent(
        &self,
        _agent_id: &str,
        _prompt: Option<&str>,
    ) -> Result<AgentSpawnResponse, String> {
        Err("Agent resumption not available in this context".into())
    }

    async fn query_agent_status(&self, _agent_id: &str) -> Result<AgentSpawnResponse, String> {
        Err("Agent status query not available in this context".into())
    }

    async fn get_agent_output(&self, _agent_id: &str) -> Result<String, String> {
        Err("Agent output not available in this context".into())
    }

    async fn background_agent(&self, _agent_id: &str) -> Result<(), String> {
        Err("Agent backgrounding not available in this context".into())
    }

    async fn resolve_skill(&self, name: &str, _args: &str) -> Result<serde_json::Value, String> {
        Err(format!("Skill resolution not available (skill: {name})"))
    }
}
