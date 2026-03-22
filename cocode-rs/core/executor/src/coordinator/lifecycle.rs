use serde::Deserialize;
use serde::Serialize;

/// Lifecycle status of a coordinated agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentLifecycleStatus {
    /// Agent is being set up (model, tools, context).
    Initializing,

    /// Agent is actively processing.
    Running,

    /// Agent is waiting for external input.
    Waiting,

    /// Agent finished successfully.
    Completed,

    /// Agent terminated with an error.
    Failed,
}

/// Unique thread identifier for an agent's execution context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadId(pub String);

impl ThreadId {
    /// Generate a new unique thread ID.
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Default for ThreadId {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "lifecycle.test.rs"]
mod tests;
