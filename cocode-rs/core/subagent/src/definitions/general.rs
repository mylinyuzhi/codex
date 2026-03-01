use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::SubagentType;
use cocode_protocol::execution::ExecutionIdentity;

/// General-purpose agent with access to all tools.
///
/// Inherits model from parent. Forks conversation context to receive
/// conversation history from the parent agent.
pub fn general_agent() -> AgentDefinition {
    AgentDefinition {
        name: SubagentType::General.as_str().to_string(),
        description: "General-purpose agent for researching complex questions, searching for \
                      code, and executing multi-step tasks."
            .to_string(),
        agent_type: SubagentType::General.as_str().to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Inherit),
        max_turns: None,
        permission_mode: None,
        fork_context: true,
        color: None,
        critical_reminder: None,
        source: AgentSource::BuiltIn,
        skills: vec![],
        background: false,
        memory: None,
        hooks: None,
        mcp_servers: None,
        isolation: None,
        use_custom_prompt: false,
    }
}

#[cfg(test)]
#[path = "general.test.rs"]
mod tests;
