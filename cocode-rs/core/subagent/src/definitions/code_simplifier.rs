use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::SubagentType;

/// Code-simplifier agent - simplifies and refines code for clarity.
///
/// Has access to all tools. Focuses on recently modified code unless
/// instructed otherwise, preserving all functionality while improving
/// readability and maintainability.
pub fn code_simplifier_agent() -> AgentDefinition {
    AgentDefinition {
        name: SubagentType::CodeSimplifier.as_str().to_string(),
        description: "Simplifies and refines code for clarity, consistency, and maintainability \
                      while preserving all functionality. Focuses on recently modified code \
                      unless instructed otherwise."
            .to_string(),
        agent_type: SubagentType::CodeSimplifier.as_str().to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        identity: None,
        max_turns: None,
        permission_mode: None,
        fork_context: false,
        color: Some("magenta".to_string()),
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
#[path = "code_simplifier.test.rs"]
mod tests;
