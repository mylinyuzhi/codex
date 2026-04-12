//! Auto-mode classification for permission decisions.
//!
//! Determines whether a tool can be used without prompting in auto mode.
//! Heuristic-based classifier matching TS yoloClassifier.ts safe-tool allowlist.
//! Future: two-stage LLM classifier with XML parsing.

use coco_types::MCP_TOOL_PREFIX;
use coco_types::ToolName;

/// Result of auto-mode classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoModeDecision {
    /// Tool is safe to run without prompting.
    Allow,
    /// Tool needs user confirmation.
    NeedsPrompt { reason: String },
}

/// Extended input for auto-mode classification.
///
/// Callers construct this from tool metadata + shell analysis results.
pub struct AutoModeInput<'a> {
    pub tool_name: &'a str,
    pub input: &'a serde_json::Value,
    pub is_read_only: bool,
    /// For Bash tools: whether the command was classified as read-only
    /// by shell-level analysis (e.g., `is_read_only_command()`).
    pub bash_is_read_only: bool,
}

/// Classify whether a tool can be used in auto mode.
///
/// Safe-tool allowlist (from TS yoloClassifier.ts):
/// - All read-only tools: allow
/// - Bash: allow if command is read-only, otherwise prompt
/// - Write/Edit: allow if path is relative or /tmp, otherwise prompt
/// - Task/Todo tools: allow (read-only side effects)
/// - Unknown tools: prompt
pub fn classify_for_auto_mode(
    tool_name: &str,
    input: &serde_json::Value,
    is_read_only: bool,
) -> AutoModeDecision {
    classify_auto_mode_extended(&AutoModeInput {
        tool_name,
        input,
        is_read_only,
        bash_is_read_only: false,
    })
}

/// Extended auto-mode classification with bash read-only awareness.
pub fn classify_auto_mode_extended(ctx: &AutoModeInput<'_>) -> AutoModeDecision {
    // Read-only tools are always safe
    if ctx.is_read_only {
        return AutoModeDecision::Allow;
    }

    let name = ctx.tool_name;

    // File I/O: check path safety
    if name == ToolName::Write.as_str() || name == ToolName::Edit.as_str() {
        return classify_file_path(name, ctx.input);
    }

    // Bash: allow read-only commands, prompt for others
    if name == ToolName::Bash.as_str() {
        return if ctx.bash_is_read_only {
            AutoModeDecision::Allow
        } else {
            AutoModeDecision::NeedsPrompt {
                reason: "bash command requires review".into(),
            }
        };
    }

    // Task/Todo management tools — safe (session-local side effects only)
    const TASK_TOOLS: &[&str] = &[
        ToolName::TaskCreate.as_str(),
        ToolName::TaskUpdate.as_str(),
        ToolName::TaskGet.as_str(),
        ToolName::TaskList.as_str(),
        ToolName::TaskStop.as_str(),
        ToolName::TaskOutput.as_str(),
        ToolName::TodoWrite.as_str(),
    ];
    if TASK_TOOLS.contains(&name) {
        return AutoModeDecision::Allow;
    }

    // Plan mode tools — safe (local state changes)
    if name == ToolName::EnterPlanMode.as_str() || name == ToolName::ExitPlanMode.as_str() {
        return AutoModeDecision::Allow;
    }

    // Agent spawning — prompt (creates sub-processes)
    const AGENT_TOOLS: &[&str] = &[
        ToolName::Agent.as_str(),
        ToolName::SendMessage.as_str(),
        ToolName::TeamCreate.as_str(),
        ToolName::TeamDelete.as_str(),
    ];
    if AGENT_TOOLS.contains(&name) {
        return AutoModeDecision::NeedsPrompt {
            reason: format!("{name} creates sub-agents"),
        };
    }

    // Web tools — prompt (network access)
    if name == ToolName::WebFetch.as_str() || name == ToolName::WebSearch.as_str() {
        return AutoModeDecision::NeedsPrompt {
            reason: format!("{name} accesses the network"),
        };
    }

    // Worktree — prompt (git operations)
    if name == ToolName::EnterWorktree.as_str() || name == ToolName::ExitWorktree.as_str() {
        return AutoModeDecision::NeedsPrompt {
            reason: "worktree operations modify git state".into(),
        };
    }

    // Scheduling — prompt (persistent side effects)
    const SCHEDULE_TOOLS: &[&str] = &[
        ToolName::CronCreate.as_str(),
        ToolName::CronDelete.as_str(),
        ToolName::RemoteTrigger.as_str(),
    ];
    if SCHEDULE_TOOLS.contains(&name) {
        return AutoModeDecision::NeedsPrompt {
            reason: format!("{name} has persistent side effects"),
        };
    }

    // MCP tools — prompt (external system interaction)
    if name.starts_with(MCP_TOOL_PREFIX) {
        return AutoModeDecision::NeedsPrompt {
            reason: format!("MCP tool {name}"),
        };
    }

    // Unknown tools — prompt
    AutoModeDecision::NeedsPrompt {
        reason: format!("unknown tool: {name}"),
    }
}

/// Classify file path safety for Write/Edit tools.
fn classify_file_path(tool_name: &str, input: &serde_json::Value) -> AutoModeDecision {
    if let Some(path) = input.get("file_path").and_then(|v| v.as_str()) {
        // Relative paths or /tmp are safe
        if !path.starts_with('/') || path.starts_with("/tmp") {
            return AutoModeDecision::Allow;
        }
    }
    AutoModeDecision::NeedsPrompt {
        reason: format!("{tool_name} to absolute path"),
    }
}

#[cfg(test)]
#[path = "auto_mode.test.rs"]
mod tests;
