//! Agent identity propagation via `tokio::task_local!`.
//!
//! Provides a mechanism for any code in the subagent call stack to query
//! the current agent context without parameter threading. The identity is
//! set at the start of agent execution via [`CURRENT_AGENT`]`.scope()`.

tokio::task_local! {
    /// Task-local storage for the current agent identity.
    ///
    /// Set via `CURRENT_AGENT.scope(identity, future)` at the start of agent
    /// execution. Query via [`current_agent()`].
    pub static CURRENT_AGENT: AgentIdentity;
}

/// Identity of a running agent, available via task-local storage.
#[derive(Debug, Clone)]
pub struct AgentIdentity {
    /// Unique identifier for this agent instance.
    pub agent_id: String,
    /// The agent type (e.g. "general-purpose", "Explore").
    pub agent_type: String,
    /// The parent agent ID, if this is a nested subagent.
    pub parent_agent_id: Option<String>,
    /// Nesting depth (0 for top-level agents).
    pub depth: i32,
    /// Display name for this agent instance.
    pub name: Option<String>,
    /// Team this agent belongs to.
    pub team_name: Option<String>,
    /// Display color for TUI rendering.
    pub color: Option<String>,
    /// Whether the agent operates in plan mode (read-only until approved).
    pub plan_mode_required: bool,
}

/// Get the current agent identity, if running inside a subagent scope.
///
/// Returns `None` if called outside any `CURRENT_AGENT.scope()`.
pub fn current_agent() -> Option<AgentIdentity> {
    CURRENT_AGENT.try_with(std::clone::Clone::clone).ok()
}

#[cfg(test)]
#[path = "identity.test.rs"]
mod tests;
