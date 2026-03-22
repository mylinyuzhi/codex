use serde::Deserialize;
use serde::Serialize;

/// Request to send input to a running agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendInputRequest {
    /// The ID of the agent to send input to.
    pub agent_id: String,

    /// The input text to deliver.
    pub input: String,
}

#[cfg(test)]
#[path = "send_input.test.rs"]
mod tests;
