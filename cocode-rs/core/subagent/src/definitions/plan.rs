use crate::definition::AgentDefinition;
use crate::definition::AgentSource;
use cocode_protocol::SubagentType;
use cocode_protocol::ToolName;
use cocode_protocol::execution::ExecutionIdentity;
use cocode_protocol::model::ModelRole;

/// Plan agent - creates plans without executing modifications (read-only).
///
/// Has access to all tools except write-oriented ones (Edit, Write, NotebookEdit).
/// Uses the Plan model role.
pub fn plan_agent() -> AgentDefinition {
    AgentDefinition {
        name: SubagentType::Plan.as_str().to_string(),
        description: "Software architect agent for designing implementation plans. Returns \
                      step-by-step plans and identifies critical files."
            .to_string(),
        agent_type: SubagentType::Plan.as_str().to_string(),
        tools: vec![],
        disallowed_tools: vec![
            ToolName::Edit.as_str().to_string(),
            ToolName::Write.as_str().to_string(),
            ToolName::NotebookEdit.as_str().to_string(),
        ],
        identity: Some(ExecutionIdentity::Role(ModelRole::Plan)),
        max_turns: None,
        permission_mode: None,
        fork_context: false,
        color: Some("blue".to_string()),
        critical_reminder: Some(
            "CRITICAL: This is a READ-ONLY planning task. Do not modify files.".to_string(),
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
#[path = "plan.test.rs"]
mod tests;
