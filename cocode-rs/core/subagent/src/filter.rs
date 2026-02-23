use crate::definition::AgentDefinition;

/// Tools that are never available to any subagent, regardless of configuration.
const SYSTEM_BLOCKED: &[&str] = &[
    "Task",
    "EnterPlanMode",
    "ExitPlanMode",
    "TaskStop",
    "AskUserQuestion",
    "EnterWorktree",
];

/// Tools safe for async/background execution (no user interaction required).
const ASYNC_SAFE_TOOLS: &[&str] = &[
    "Read",
    "Edit",
    "Write",
    "Glob",
    "Grep",
    "Bash",
    "WebFetch",
    "WebSearch",
    "NotebookEdit",
    "TaskOutput",
    "Task", // Task is async-safe when explicitly allowed via Task(type) syntax
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
///    overrides the Task block with type restrictions.
/// 2. **Definition allow-list** - if `definition.tools` is non-empty, only
///    those tools are retained. `Task(type1, type2)` normalizes to `Task`.
/// 3. **Definition deny-list** - tools in `definition.disallowed_tools` are
///    removed. Also supports `Task(type)` normalization.
/// 4. **Background filter** - when `background` is `true`, only tools in
///    `ASYNC_SAFE_TOOLS` are retained.
pub fn filter_tools_for_agent(
    all_tools: &[String],
    definition: &AgentDefinition,
    background: bool,
) -> ToolFilterResult {
    // Parse Task(type) restrictions from allow-list
    let task_type_restrictions = extract_task_restrictions(&definition.tools);
    let has_task_override = task_type_restrictions.is_some();

    // Layer 1: system-blocked tools are removed.
    // Exception: if Task(type) is in the allow-list, "Task" is exempt from blocking.
    let mut result: Vec<String> = all_tools
        .iter()
        .filter(|t| {
            let name = t.as_str();
            if name == "Task" && has_task_override {
                return true; // Task explicitly allowed via Task(type) syntax
            }
            !SYSTEM_BLOCKED.contains(&name)
        })
        .cloned()
        .collect();

    // Layer 2: apply allow-list if provided.
    if !definition.tools.is_empty() {
        let normalized_tools: Vec<String> = definition
            .tools
            .iter()
            .map(|t| normalize_tool_name(t))
            .collect();
        result.retain(|t| normalized_tools.contains(t));
    }

    // Layer 3: apply deny-list.
    if !definition.disallowed_tools.is_empty() {
        let normalized_deny: Vec<String> = definition
            .disallowed_tools
            .iter()
            .map(|t| normalize_tool_name(t))
            .collect();
        result.retain(|t| !normalized_deny.contains(t));
    }

    // Layer 4: background agents can only use async-safe tools.
    if background {
        result.retain(|t| ASYNC_SAFE_TOOLS.contains(&t.as_str()));
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
    if !trimmed.starts_with("Task(") || !trimmed.ends_with(')') {
        return None;
    }
    let inner = &trimmed[5..trimmed.len() - 1];
    let types: Vec<String> = inner
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if types.is_empty() { None } else { Some(types) }
}

/// Normalize a tool name by stripping `Task(...)` to just `Task`.
fn normalize_tool_name(tool: &str) -> String {
    if tool.trim().starts_with("Task(") && tool.trim().ends_with(')') {
        "Task".to_string()
    } else {
        tool.to_string()
    }
}

#[cfg(test)]
#[path = "filter.test.rs"]
mod tests;
