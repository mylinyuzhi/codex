use crate::definition::AgentDefinition;
use cocode_protocol::execution::ExecutionIdentity;

/// Guide agent - reads and navigates documentation and code.
/// Inherits model from parent.
pub fn guide_agent() -> AgentDefinition {
    AgentDefinition {
        name: "guide".to_string(),
        description: "Guided reading agent for documentation and code navigation".to_string(),
        agent_type: "guide".to_string(),
        tools: vec!["Glob".to_string(), "Grep".to_string(), "Read".to_string()],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Inherit),
        max_turns: Some(15),
        permission_mode: Some(cocode_protocol::PermissionMode::DontAsk),
    }
}

#[cfg(test)]
#[path = "guide.test.rs"]
mod tests;
