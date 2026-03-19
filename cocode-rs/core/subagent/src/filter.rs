use std::borrow::Cow;

use crate::definition::AgentDefinition;
use cocode_protocol::PermissionMode;
use cocode_protocol::ToolName;

/// Tools that are never available to any subagent, regardless of configuration.
const SYSTEM_BLOCKED: &[&str] = &[
    ToolName::Task.as_str(),
    ToolName::EnterPlanMode.as_str(),
    ToolName::ExitPlanMode.as_str(),
    ToolName::TaskStop.as_str(),
    ToolName::AskUserQuestion.as_str(),
    ToolName::EnterWorktree.as_str(),
    ToolName::ExitWorktree.as_str(),
    ToolName::CronCreate.as_str(),
    ToolName::CronDelete.as_str(),
    ToolName::TeamCreate.as_str(),
    ToolName::TeamDelete.as_str(),
    ToolName::SendMessage.as_str(),
];

/// Tools safe for async/background execution (no user interaction required).
const ASYNC_SAFE_TOOLS: &[&str] = &[
    ToolName::Read.as_str(),
    ToolName::Edit.as_str(),
    ToolName::Write.as_str(),
    ToolName::Glob.as_str(),
    ToolName::Grep.as_str(),
    ToolName::Bash.as_str(),
    ToolName::WebFetch.as_str(),
    ToolName::WebSearch.as_str(),
    ToolName::NotebookEdit.as_str(),
    ToolName::TaskOutput.as_str(),
    ToolName::Task.as_str(), // Task is async-safe when explicitly allowed via Task(type) syntax
    ToolName::TaskCreate.as_str(),
    ToolName::TaskUpdate.as_str(),
    ToolName::TaskGet.as_str(),
    ToolName::TaskList.as_str(),
    ToolName::CronList.as_str(),
    ToolName::Skill.as_str(),
    ToolName::McpSearch.as_str(),
    ToolName::TodoWrite.as_str(),
    ToolName::EnterWorktree.as_str(),
    ToolName::ExitWorktree.as_str(),
];

/// Result of tool filtering, including any `Task(type)` restrictions.
#[derive(Debug, Clone)]
pub struct ToolFilterResult {
    /// The filtered tool names.
    pub tools: Vec<String>,
    /// If the definition's allow-list contained `Task(type1, type2, ...)`,
    /// this holds the allowed subagent types. `None` means no restriction
    /// (but Task is normally system-blocked unless explicitly allowed).
    pub task_type_restrictions: Option<Vec<String>>,
}

/// Apply four-layer tool filtering for a subagent.
///
/// Filtering is applied in order:
///
/// 1. **System blocked** - tools in `SYSTEM_BLOCKED` are always removed,
///    UNLESS the allow-list explicitly includes `Task(type1, type2)` which
///    overrides the Task block with type restrictions. MCP tools (`mcp__*`)
///    bypass all filtering unconditionally. `ExitPlanMode` is allowed when
///    `permission_mode` is `Plan`.
/// 2. **Definition allow-list** - if `definition.tools` is non-empty, only
///    those tools are retained. `Task(type1, type2)` normalizes to `Task`.
///    MCP tools bypass allow-list filtering.
/// 3. **Definition deny-list** - tools in `definition.disallowed_tools` are
///    removed. Also supports `Task(type)` normalization.
/// 4. **Background filter** - when `background` is `true`, only tools in
///    `ASYNC_SAFE_TOOLS` are retained. MCP tools bypass background filtering.
pub fn filter_tools_for_agent(
    all_tools: &[String],
    definition: &AgentDefinition,
    background: bool,
    permission_mode: Option<&PermissionMode>,
) -> ToolFilterResult {
    // Parse Task(type) restrictions from allow-list
    let task_type_restrictions = extract_task_restrictions(&definition.tools);
    let has_task_override = task_type_restrictions.is_some();

    // Layer 1: system-blocked tools are removed.
    // Exceptions:
    // - MCP tools (mcp__*) bypass all filtering unconditionally
    // - Task(type) in allow-list exempts Task from blocking
    // - ExitPlanMode is allowed when permission_mode is Plan
    let mut result: Vec<String> = all_tools
        .iter()
        .filter(|t| {
            let name = t.as_str();
            // MCP tools bypass all filtering unconditionally
            if name.starts_with("mcp__") {
                return true;
            }
            // ExitPlanMode is allowed in Plan permission mode
            if name == ToolName::ExitPlanMode.as_str()
                && permission_mode == Some(&PermissionMode::Plan)
            {
                return true;
            }
            if name == ToolName::Task.as_str() && has_task_override {
                return true; // Task explicitly allowed via Task(type) syntax
            }
            !SYSTEM_BLOCKED.contains(&name)
        })
        .cloned()
        .collect();

    // Layer 2: apply allow-list if provided.
    if !definition.tools.is_empty() {
        let normalized_tools: Vec<Cow<'_, str>> = definition
            .tools
            .iter()
            .map(|t| normalize_tool_name(t))
            .collect();
        result.retain(|t| {
            // MCP tools bypass allow-list filtering
            if t.starts_with("mcp__") {
                return true;
            }
            normalized_tools.iter().any(|nt| nt.as_ref() == t.as_str())
        });
    }

    // Layer 3: apply deny-list.
    if !definition.disallowed_tools.is_empty() {
        let normalized_deny: Vec<Cow<'_, str>> = definition
            .disallowed_tools
            .iter()
            .map(|t| normalize_tool_name(t))
            .collect();
        result.retain(|t| {
            // MCP tools bypass deny-list filtering
            if t.starts_with("mcp__") {
                return true;
            }
            !normalized_deny.iter().any(|nd| nd.as_ref() == t.as_str())
        });
    }

    // Layer 4: background agents can only use async-safe tools.
    if background {
        result.retain(|t| {
            // MCP tools bypass background filtering
            if t.starts_with("mcp__") {
                return true;
            }
            ASYNC_SAFE_TOOLS.contains(&t.as_str())
        });
    }

    ToolFilterResult {
        tools: result,
        task_type_restrictions,
    }
}

/// Extract `Task(type1, type2)` restrictions from a tools list.
///
/// Returns `Some(types)` if any `Task(...)` entry is found, `None` otherwise.
pub fn extract_task_restrictions(tools: &[String]) -> Option<Vec<String>> {
    for tool in tools {
        if let Some(types) = parse_task_restriction(tool) {
            return Some(types);
        }
    }
    None
}

/// Parse a single tool entry for `Task(type1, type2)` syntax.
///
/// Returns `Some(vec!["type1", "type2"])` if the entry matches, `None` otherwise.
fn parse_task_restriction(tool: &str) -> Option<Vec<String>> {
    let trimmed = tool.trim();
    let prefix = format!("{}(", ToolName::Task.as_str());
    if !trimmed.starts_with(&prefix) || !trimmed.ends_with(')') {
        return None;
    }
    let inner = &trimmed[prefix.len()..trimmed.len() - 1];
    let types: Vec<String> = inner
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if types.is_empty() { None } else { Some(types) }
}

/// Normalize a tool name by stripping `Task(...)` to just `Task`.
fn normalize_tool_name(tool: &str) -> Cow<'_, str> {
    if tool.trim().starts_with("Task(") && tool.trim().ends_with(')') {
        Cow::Owned(ToolName::Task.as_str().to_string())
    } else {
        Cow::Borrowed(tool)
    }
}

#[cfg(test)]
#[path = "filter.test.rs"]
mod tests;
