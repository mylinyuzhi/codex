use cocode_protocol::execution::ExecutionIdentity;
use serde::Deserialize;
use serde::Serialize;

/// Input parameters for spawning a new subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnInput {
    /// The agent type to spawn (must match a registered `AgentDefinition`).
    pub agent_type: String,

    /// The prompt or task description for the subagent.
    pub prompt: String,

    /// Model selection identity for this spawn.
    ///
    /// Determines how the model is resolved:
    /// - `Role(ModelRole)`: Use the model configured for that role
    /// - `Spec(ModelSpec)`: Use a specific provider/model
    /// - `Inherit`: Use the parent agent's model
    /// - `None`: Fall back to definition's identity or parent model
    #[serde(default)]
    pub identity: Option<ExecutionIdentity>,

    /// Override the maximum number of turns.
    #[serde(default)]
    pub max_turns: Option<i32>,

    /// Whether this agent should run in the background.
    ///
    /// - `Some(true/false)`: Explicitly set by the model.
    /// - `None`: Deferred to the agent definition's `background` default.
    #[serde(default)]
    pub run_in_background: Option<bool>,

    /// Override the allowed tools for this spawn.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    /// Agent ID to resume from a previous invocation.
    ///
    /// When set, the agent loads the prior output and prepends it as context
    /// to the prompt, allowing continuation of a previous session.
    #[serde(default)]
    pub resume_from: Option<String>,

    /// Display name for the spawned agent.
    #[serde(default)]
    pub name: Option<String>,

    /// Team to auto-join the agent to after spawn.
    #[serde(default)]
    pub team_name: Option<String>,

    /// Agent execution mode (normal, plan, auto).
    #[serde(default)]
    pub mode: Option<String>,

    /// Working directory override for the spawned agent.
    #[serde(default)]
    pub cwd: Option<String>,

    /// Isolation mode override (e.g. "worktree").
    #[serde(default)]
    pub isolation_override: Option<String>,

    /// Short description of what the agent will do (for TUI display).
    #[serde(default)]
    pub description: Option<String>,
}

#[cfg(test)]
#[path = "spawn.test.rs"]
mod tests;
