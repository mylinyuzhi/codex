use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::SubagentType;
use cocode_protocol::ToolName;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

/// Statusline agent - makes small targeted edits to status/progress displays.
/// Uses the Fast model role for efficiency.
pub fn statusline_agent() -> AgentDefinition {
    AgentDefinition {
        name: SubagentType::Statusline.as_str().to_string(),
        description: "Use this agent to configure the user's status line setting.".to_string(),
        agent_type: SubagentType::Statusline.as_str().to_string(),
        tools: vec![
            ToolName::Read.as_str().to_string(),
            ToolName::Edit.as_str().to_string(),
        ],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Role(ModelRole::Fast)),
        max_turns: Some(5),
        permission_mode: None,
        fork_context: false,
        color: Some("orange".to_string()),
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
#[path = "statusline.test.rs"]
mod tests;
