use crate::definition::AgentDefinition;
use cocode_protocol::execution::ExecutionIdentity;

/// Statusline agent - makes small targeted edits to status/progress displays.
/// Inherits model from parent.
pub fn statusline_agent() -> AgentDefinition {
    AgentDefinition {
        name: "statusline".to_string(),
        description: "Lightweight agent for status line and progress display updates".to_string(),
        agent_type: "statusline".to_string(),
        tools: vec!["Read".to_string(), "Edit".to_string()],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Inherit),
        max_turns: Some(5),
        permission_mode: None,
    }
}

#[cfg(test)]
#[path = "statusline.test.rs"]
mod tests;
