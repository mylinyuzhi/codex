use serde::Deserialize;
use serde::Serialize;

/// Request to wait for an agent to complete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitRequest {
    /// The ID of the agent to wait for.
    pub agent_id: String,

    /// Optional timeout in seconds. `None` means wait indefinitely.
    #[serde(default)]
    pub timeout_secs: Option<i64>,
}

#[cfg(test)]
#[path = "wait.test.rs"]
mod tests;
