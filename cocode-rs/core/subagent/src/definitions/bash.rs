use crate::definition::AgentDefinition;
use cocode_protocol::execution::ExecutionIdentity;

/// Bash agent - executes shell commands.
pub fn bash_agent() -> AgentDefinition {
    AgentDefinition {
        name: "bash".to_string(),
        description: "Executes shell commands via Bash".to_string(),
        agent_type: "bash".to_string(),
        tools: vec!["Bash".to_string()],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Inherit),
        max_turns: Some(10),
        permission_mode: None,
    }
}

#[cfg(test)]
#[path = "bash.test.rs"]
mod tests;
