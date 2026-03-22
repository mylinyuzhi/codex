use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::PermissionMode;
use cocode_protocol::SubagentType;
use cocode_protocol::ToolName;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

/// Guide agent - reads and navigates documentation and code (read-only).
///
/// Has access to Glob, Grep, Read, WebFetch, WebSearch.
/// Uses the Fast model role and bypasses permission checks since it's read-only.
pub fn guide_agent() -> AgentDefinition {
    AgentDefinition {
        name: SubagentType::Guide.as_str().to_string(),
        description: "Use this agent when the user asks questions about Claude Code features, \
                      hooks, MCP servers, settings, Agent SDK, or Claude API."
            .to_string(),
        agent_type: SubagentType::Guide.as_str().to_string(),
        tools: vec![
            ToolName::Glob.as_str().to_string(),
            ToolName::Grep.as_str().to_string(),
            ToolName::Read.as_str().to_string(),
            ToolName::WebFetch.as_str().to_string(),
            ToolName::WebSearch.as_str().to_string(),
        ],
        disallowed_tools: vec![],
        identity: Some(ExecutionIdentity::Role(ModelRole::Fast)),
        max_turns: Some(15),
        permission_mode: Some(PermissionMode::Bypass),
        fork_context: false,
        color: Some("green".to_string()),
        critical_reminder: Some(
            "CRITICAL: This is a READ-ONLY help task. Do not modify any files.".to_string(),
        ),
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
#[path = "guide.test.rs"]
mod tests;
