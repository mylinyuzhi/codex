use crate::definition::AgentDefinition;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

/// Plan agent - creates plans without executing modifications.
/// Uses the Plan model role if configured, otherwise inherits from parent.
pub fn plan_agent() -> AgentDefinition {
    AgentDefinition {
        name: "plan".to_string(),
        description: "Planning agent that reasons about tasks without making changes".to_string(),
        agent_type: "plan".to_string(),
        tools: vec![],
        disallowed_tools: vec!["Task".to_string(), "Edit".to_string(), "Write".to_string()],
        identity: Some(ExecutionIdentity::Role(ModelRole::Plan)),
        max_turns: None,
        permission_mode: None,
    }
}

#[cfg(test)]
#[path = "plan.test.rs"]
mod tests;
