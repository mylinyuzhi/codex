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
];

/// Apply four-layer tool filtering for a subagent.
///
/// Filtering is applied in order:
///
/// 1. **System blocked** - tools in `SYSTEM_BLOCKED` are always removed.
/// 2. **Definition allow-list** - if `definition.tools` is non-empty, only
///    those tools are retained.
/// 3. **Definition deny-list** - tools in `definition.disallowed_tools` are
///    removed.
/// 4. **Background filter** - when `background` is `true`, only tools in
///    `ASYNC_SAFE_TOOLS` are retained.
pub fn filter_tools_for_agent(
    all_tools: &[String],
    definition: &AgentDefinition,
    background: bool,
) -> Vec<String> {
    let mut result: Vec<String> = all_tools
        .iter()
        .filter(|t| !SYSTEM_BLOCKED.contains(&t.as_str()))
        .cloned()
        .collect();

    // Layer 2: apply allow-list if provided.
    if !definition.tools.is_empty() {
        result.retain(|t| definition.tools.contains(t));
    }

    // Layer 3: apply deny-list.
    if !definition.disallowed_tools.is_empty() {
        result.retain(|t| !definition.disallowed_tools.contains(t));
    }

    // Layer 4: background agents can only use async-safe tools.
    if background {
        result.retain(|t| ASYNC_SAFE_TOOLS.contains(&t.as_str()));
    }

    result
}

#[cfg(test)]
#[path = "filter.test.rs"]
mod tests;
