use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::PermissionMode;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

/// Explore agent - fast codebase exploration (read-only).
///
/// Has access to all tools except write-oriented ones (Edit, Write, NotebookEdit).
/// Uses the Explore model role and bypasses permission checks since it's read-only.
pub fn explore_agent() -> AgentDefinition {
    AgentDefinition {
        name: "explore".to_string(),
        description: "Fast agent specialized for exploring codebases. Use for finding files, \
                      searching code, or answering codebase questions."
            .to_string(),
        agent_type: "explore".to_string(),
        tools: vec![],
        disallowed_tools: vec![
            cocode_protocol::tools::EDIT.to_string(),
            cocode_protocol::tools::WRITE.to_string(),
            cocode_protocol::tools::NOTEBOOK_EDIT.to_string(),
        ],
        identity: Some(ExecutionIdentity::Role(ModelRole::Explore)),
        max_turns: Some(20),
        permission_mode: Some(PermissionMode::Bypass),
        fork_context: false,
        color: Some("cyan".to_string()),
        critical_reminder: Some(
            "CRITICAL: This is a READ-ONLY exploration task. Do not modify files.".to_string(),
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
#[path = "explore.test.rs"]
mod tests;
