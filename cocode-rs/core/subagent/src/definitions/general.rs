use crate::definition::AgentDefinition;
use cocode_protocol::execution::ExecutionIdentity;

/// General-purpose agent with access to all tools.
/// Inherits model from parent.
pub fn general_agent() -> AgentDefinition {
    AgentDefinition {
        name: "general".to_string(),
        description: "General-purpose coding agent with access to all tools".to_string(),
        agent_type: "general".to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Inherit),
        max_turns: None,
        permission_mode: None,
    }
}

#[cfg(test)]
#[path = "general.test.rs"]
mod tests;
