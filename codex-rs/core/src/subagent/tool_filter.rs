//! Three-tier tool restriction system for subagents.

use super::definition::AgentDefinition;
use super::definition::AgentSource;
use super::definition::ToolAccess;
use std::collections::HashSet;

/// Tools blocked for ALL subagents (recursive/dangerous).
pub const ALWAYS_BLOCKED_TOOLS: &[&str] = &[
    "Task",       // Prevent recursive subagent spawning
    "TaskOutput", // Associated with Task
    "TodoWrite",  // Main agent responsibility only
];

/// Additional tools blocked for non-builtin agents (security).
/// These tools can modify the filesystem or execute commands.
pub const NON_BUILTIN_BLOCKED_TOOLS: &[&str] = &[
    "Write",        // File creation
    "Bash",         // Shell execution (high risk)
    "NotebookEdit", // Notebook modification
    "Edit",         // File editing (conservative)
];

/// Tools safe for async/background execution.
pub const ASYNC_SAFE_TOOLS: &[&str] = &[
    "Read",
    "Edit",
    "Grep",
    "WebSearch",
    "Glob",
    "Bash",
    "Skill",
    "SlashCommand",
    "WebFetch",
];

/// Tool filter for subagent execution.
///
/// Tool access is determined solely by the agent's definition (ToolAccess + disallowed_tools)
/// and security tiers (ALWAYS_BLOCKED, NON_BUILTIN_BLOCKED). No parent tool intersection
/// is performed - subagents can use any tool their definition allows, independent of
/// what tools the parent session has available.
#[derive(Debug, Clone)]
pub struct ToolFilter {
    /// Allowed tools based on definition.
    allowed_tools: ToolAccess,
    /// Explicitly disallowed tools.
    disallowed_tools: HashSet<String>,
    /// Whether this is a builtin agent.
    is_builtin: bool,
    /// Whether running in async/background mode.
    is_async: bool,
}

impl ToolFilter {
    /// Create a new tool filter for an agent.
    ///
    /// Tool filtering is based solely on the agent's definition - no parent tool
    /// intersection is performed.
    pub fn new(definition: &AgentDefinition) -> Self {
        Self {
            allowed_tools: definition.tools.clone(),
            disallowed_tools: definition.disallowed_tools.iter().cloned().collect(),
            is_builtin: definition.source == AgentSource::Builtin,
            is_async: false,
        }
    }

    /// Set async mode (restricts to ASYNC_SAFE_TOOLS).
    pub fn with_async(mut self, is_async: bool) -> Self {
        self.is_async = is_async;
        self
    }

    /// Check if a tool is allowed for this subagent.
    pub fn is_allowed(&self, tool_name: &str) -> bool {
        // Tier 1: Always blocked tools
        if ALWAYS_BLOCKED_TOOLS.contains(&tool_name) {
            return false;
        }

        // Tier 2: Non-builtin blocked tools
        if !self.is_builtin && NON_BUILTIN_BLOCKED_TOOLS.contains(&tool_name) {
            return false;
        }

        // Tier 3: Agent-specific disallowed tools
        if self.disallowed_tools.contains(tool_name) {
            return false;
        }

        // Async mode: Only allow ASYNC_SAFE_TOOLS
        if self.is_async && !ASYNC_SAFE_TOOLS.contains(&tool_name) {
            return false;
        }

        // Check agent's allowed tools (no parent tool intersection)
        self.allowed_tools.allows(tool_name)
    }

    /// Filter a list of tool names, returning only allowed ones.
    pub fn filter_tools<'a>(&self, tools: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
        tools.filter(|t| self.is_allowed(t)).collect()
    }

    /// Get the reason a tool is blocked (for error messages).
    pub fn rejection_reason(&self, tool_name: &str) -> Option<String> {
        if ALWAYS_BLOCKED_TOOLS.contains(&tool_name) {
            return Some(format!(
                "Tool '{tool_name}' is always blocked in subagent context (recursive/dangerous)"
            ));
        }

        if !self.is_builtin && NON_BUILTIN_BLOCKED_TOOLS.contains(&tool_name) {
            return Some(format!(
                "Tool '{tool_name}' is blocked for non-builtin agents (security restriction)"
            ));
        }

        if self.disallowed_tools.contains(tool_name) {
            return Some(format!(
                "Tool '{tool_name}' is explicitly disallowed for this agent"
            ));
        }

        if self.is_async && !ASYNC_SAFE_TOOLS.contains(&tool_name) {
            return Some(format!(
                "Tool '{tool_name}' is not safe for async/background execution"
            ));
        }

        // No parent tool intersection check - subagent tools are independent

        if !self.allowed_tools.allows(tool_name) {
            return Some(format!(
                "Tool '{tool_name}' is not in this agent's allowed tools list"
            ));
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::definition::AgentRunConfig;
    use crate::subagent::definition::ModelConfig;
    use crate::subagent::definition::PromptConfig;

    fn test_agent(source: AgentSource, tools: ToolAccess) -> AgentDefinition {
        AgentDefinition {
            agent_type: "test".to_string(),
            display_name: None,
            when_to_use: None,
            tools,
            disallowed_tools: vec![],
            source,
            model_config: ModelConfig::default(),
            fork_context: false,
            prompt_config: PromptConfig::default(),
            run_config: AgentRunConfig::default(),
            input_config: None,
            output_config: None,
            approval_mode: Default::default(),
            critical_system_reminder: None,
        }
    }

    #[test]
    fn test_always_blocked() {
        let agent = test_agent(AgentSource::Builtin, ToolAccess::All);
        let filter = ToolFilter::new(&agent);

        assert!(!filter.is_allowed("Task"));
        assert!(!filter.is_allowed("TaskOutput"));
        assert!(!filter.is_allowed("TodoWrite"));
    }

    #[test]
    fn test_non_builtin_blocked() {
        let builtin = test_agent(AgentSource::Builtin, ToolAccess::All);
        let user = test_agent(AgentSource::User, ToolAccess::All);

        let builtin_filter = ToolFilter::new(&builtin);
        let user_filter = ToolFilter::new(&user);

        // Builtin can use Write and Edit
        assert!(builtin_filter.is_allowed("Write"));
        assert!(builtin_filter.is_allowed("Edit"));
        // User cannot
        assert!(!user_filter.is_allowed("Write"));
        assert!(!user_filter.is_allowed("Edit"));
        assert!(!user_filter.is_allowed("Bash"));
        assert!(!user_filter.is_allowed("NotebookEdit"));
    }

    #[test]
    fn test_async_safe_tools() {
        let agent = test_agent(AgentSource::Builtin, ToolAccess::All);
        let filter = ToolFilter::new(&agent).with_async(true);

        assert!(filter.is_allowed("Read"));
        assert!(filter.is_allowed("Grep"));
        assert!(!filter.is_allowed("SomeCustomTool"));
    }

    #[test]
    fn test_tool_access_list() {
        let agent = test_agent(
            AgentSource::Builtin,
            ToolAccess::List(vec!["Read".to_string(), "Glob".to_string()]),
        );
        let filter = ToolFilter::new(&agent);

        assert!(filter.is_allowed("Read"));
        assert!(filter.is_allowed("Glob"));
        assert!(!filter.is_allowed("Write"));
    }

    #[test]
    fn test_rejection_reason() {
        let agent = test_agent(AgentSource::User, ToolAccess::All);
        let filter = ToolFilter::new(&agent);

        let reason = filter.rejection_reason("Task");
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("always blocked"));
    }
}
