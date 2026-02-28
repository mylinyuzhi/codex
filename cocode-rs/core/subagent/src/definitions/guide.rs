use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::PermissionMode;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

/// Guide agent - reads and navigates documentation and code (read-only).
///
/// Has access to Glob, Grep, Read, WebFetch, WebSearch.
/// Uses the Fast model role and bypasses permission checks since it's read-only.
pub fn guide_agent() -> AgentDefinition {
    AgentDefinition {
        name: "guide".to_string(),
        description: "Use this agent when the user asks questions about Claude Code features, \
                      hooks, MCP servers, settings, Agent SDK, or Claude API."
            .to_string(),
        agent_type: "guide".to_string(),
        tools: vec![
            cocode_protocol::tools::GLOB.to_string(),
            cocode_protocol::tools::GREP.to_string(),
            cocode_protocol::tools::READ.to_string(),
            cocode_protocol::tools::WEB_FETCH.to_string(),
            cocode_protocol::tools::WEB_SEARCH.to_string(),
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
