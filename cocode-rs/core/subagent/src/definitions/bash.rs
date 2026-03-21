use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::SubagentType;
use cocode_protocol::ToolName;
use cocode_protocol::execution::ExecutionIdentity;

/// Bash agent - executes shell commands.
pub fn bash_agent() -> AgentDefinition {
    AgentDefinition {
        name: SubagentType::Bash.as_str().to_string(),
        description: "Command execution specialist for running bash commands. Use for git \
                      operations, command execution, and terminal tasks."
            .to_string(),
        agent_type: SubagentType::Bash.as_str().to_string(),
        tools: vec![ToolName::Bash.as_str().to_string()],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Inherit),
        max_turns: Some(10),
        permission_mode: None,
        fork_context: false,
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
#[path = "bash.test.rs"]
mod tests;
