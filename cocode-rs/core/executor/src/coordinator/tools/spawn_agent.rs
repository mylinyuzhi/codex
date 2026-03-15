use serde::Deserialize;
use serde::Serialize;

/// Input for the spawn_agent tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnAgentInput {
    /// Model to use for the spawned agent.
    pub model: String,

    /// Initial prompt or task description.
    pub prompt: String,

    /// List of tools to make available.
    #[serde(default)]
    pub tools: Vec<String>,

    /// Maximum number of turns the agent may execute.
    #[serde(default)]
    pub max_turns: Option<i32>,
}

#[cfg(test)]
#[path = "spawn_agent.test.rs"]
mod tests;
