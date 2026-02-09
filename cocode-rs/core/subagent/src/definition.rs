use cocode_protocol::PermissionMode;
use cocode_protocol::execution::ExecutionIdentity;
use serde::Deserialize;
use serde::Serialize;

/// Declarative definition of a subagent type.
///
/// Each definition specifies the agent's name, description, allowed/disallowed
/// tools, and optional model and turn limit overrides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique name for this agent type (e.g. "bash", "explore").
    pub name: String,

    /// Human-readable description of the agent's purpose.
    pub description: String,

    /// Agent type identifier used for spawning.
    pub agent_type: String,

    /// Allowed tools (empty means all tools are available).
    #[serde(default)]
    pub tools: Vec<String>,

    /// Tools explicitly denied to this agent.
    #[serde(default)]
    pub disallowed_tools: Vec<String>,

    /// Model selection identity for this agent type.
    ///
    /// Determines how the model is resolved:
    /// - `Role(ModelRole)`: Use the model configured for that role
    /// - `Spec(ModelSpec)`: Use a specific provider/model
    /// - `Inherit`: Use the parent agent's model
    /// - `None`: Fall back to parent model (same as Inherit)
    #[serde(default)]
    pub identity: Option<ExecutionIdentity>,

    /// Override the maximum number of turns for this agent.
    #[serde(default)]
    pub max_turns: Option<i32>,

    /// Override the permission mode for this subagent.
    ///
    /// When set, the subagent uses this permission mode instead of
    /// inheriting the parent's mode. For example, a "guide" agent
    /// that only reads docs might use `DontAsk` to auto-deny unknown
    /// operations, while a "bash" agent uses `Default`.
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
}

#[cfg(test)]
#[path = "definition.test.rs"]
mod tests;
