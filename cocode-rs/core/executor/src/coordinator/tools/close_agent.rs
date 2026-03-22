use serde::Deserialize;
use serde::Serialize;

/// Request to close and clean up an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseAgentRequest {
    /// The ID of the agent to close.
    pub agent_id: String,

    /// Whether to force-close even if the agent is still running.
    #[serde(default)]
    pub force: bool,
}

#[cfg(test)]
#[path = "close_agent.test.rs"]
mod tests;
