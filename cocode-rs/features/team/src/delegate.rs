//! Delegate mode for team leads.
//!
//! When a lead agent enters delegate mode, it restricts itself to
//! coordination-only tools, forcing all implementation work to be
//! delegated to teammates.
//!
//! Aligned with Claude Code's delegate mode where the lead uses only
//! coordination tools: TeamCreate, TeamDelete, SendMessage,
//! TaskCreate, TaskGet, TaskUpdate, TaskList, and Task (spawn agent).

use cocode_protocol::ToolName;
use serde::Deserialize;
use serde::Serialize;

// ============================================================================
// Delegate Mode State
// ============================================================================

/// Delegate mode state for a team lead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateModeState {
    /// Whether delegate mode is active.
    pub active: bool,
    /// Team name this mode applies to.
    pub team_name: String,
    /// Agent ID of the lead.
    pub agent_id: String,
}

// ============================================================================
// Coordination Tool Set
// ============================================================================

/// The canonical set of tools available in delegate mode.
///
/// In delegate mode, the lead agent can only use these coordination tools.
/// All implementation tools (Read, Write, Edit, Bash, Grep, Glob, etc.)
/// are removed.
pub const DELEGATE_MODE_TOOLS: &[&str] = &[
    ToolName::TeamCreate.as_str(),
    ToolName::TeamDelete.as_str(),
    ToolName::SendMessage.as_str(),
    ToolName::TaskCreate.as_str(),
    ToolName::TaskGet.as_str(),
    ToolName::TaskUpdate.as_str(),
    ToolName::TaskList.as_str(),
    ToolName::Task.as_str(),
];

/// Check if a tool is allowed in delegate mode.
pub fn is_delegate_tool(tool_name: &str) -> bool {
    DELEGATE_MODE_TOOLS.contains(&tool_name)
}

/// Filter a list of tool names to only those allowed in delegate mode.
pub fn filter_for_delegate_mode(tools: &[String]) -> Vec<String> {
    tools
        .iter()
        .filter(|t| is_delegate_tool(t))
        .cloned()
        .collect()
}

#[cfg(test)]
#[path = "delegate.test.rs"]
mod tests;
