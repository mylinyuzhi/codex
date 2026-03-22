//! Tool name definitions for the cocode ecosystem.
//!
//! Provides a type-safe enum for builtin tool names. MCP tools are
//! dynamically discovered and not represented here.

use serde::Deserialize;
use serde::Serialize;

/// Builtin tool names with type-safe identifiers.
///
/// Each variant represents a builtin tool with its string name
/// accessible via `as_str()`. MCP tools are not represented here
/// as they are dynamically discovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolName {
    // File operations
    Read,
    ReadManyFiles,
    Glob,
    Grep,
    Edit,
    Write,
    LS,

    // Shell/Command execution
    Bash,
    Shell,

    // Task management
    Task,
    TaskOutput,
    TaskStop,

    // Structured task management
    TaskCreate,
    TaskUpdate,
    TaskGet,
    TaskList,

    // Planning and interaction
    EnterPlanMode,
    ExitPlanMode,
    AskUserQuestion,
    TodoWrite,

    // Worktree management
    EnterWorktree,
    ExitWorktree,

    // Cron/Scheduling
    CronCreate,
    CronDelete,
    CronList,

    // Team/Collaboration
    TeamCreate,
    TeamDelete,
    SendMessage,

    // Web tools
    WebFetch,
    WebSearch,

    // Language/Skill tools
    Skill,
    Lsp,
    NotebookEdit,
    SmartEdit,

    // Patch tool
    ApplyPatch,

    // MCP tools
    McpSearch,
}

impl ToolName {
    /// Get the string name for this tool.
    #[inline]
    pub const fn as_str(&self) -> &'static str {
        match self {
            ToolName::Read => "Read",
            ToolName::ReadManyFiles => "ReadManyFiles",
            ToolName::Glob => "Glob",
            ToolName::Grep => "Grep",
            ToolName::Edit => "Edit",
            ToolName::Write => "Write",
            ToolName::LS => "LS",
            ToolName::Bash => "Bash",
            ToolName::Shell => "shell",
            ToolName::Task => "Task",
            ToolName::TaskOutput => "TaskOutput",
            ToolName::TaskStop => "TaskStop",
            ToolName::TaskCreate => "TaskCreate",
            ToolName::TaskUpdate => "TaskUpdate",
            ToolName::TaskGet => "TaskGet",
            ToolName::TaskList => "TaskList",
            ToolName::EnterPlanMode => "EnterPlanMode",
            ToolName::ExitPlanMode => "ExitPlanMode",
            ToolName::AskUserQuestion => "AskUserQuestion",
            ToolName::TodoWrite => "TodoWrite",
            ToolName::EnterWorktree => "EnterWorktree",
            ToolName::ExitWorktree => "ExitWorktree",
            ToolName::CronCreate => "CronCreate",
            ToolName::CronDelete => "CronDelete",
            ToolName::CronList => "CronList",
            ToolName::TeamCreate => "TeamCreate",
            ToolName::TeamDelete => "TeamDelete",
            ToolName::SendMessage => "SendMessage",
            ToolName::WebFetch => "WebFetch",
            ToolName::WebSearch => "WebSearch",
            ToolName::Skill => "Skill",
            ToolName::Lsp => "Lsp",
            ToolName::NotebookEdit => "NotebookEdit",
            ToolName::SmartEdit => "SmartEdit",
            ToolName::ApplyPatch => "apply_patch",
            ToolName::McpSearch => "MCPSearch",
        }
    }

    /// Parse from a string, returns None for unknown/MCP tools.
    ///
    /// Derives mappings from `as_str()` via `ALL`, so adding a new variant
    /// only requires updating `as_str()` and `ALL` — not a third match.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        // Handle aliases that don't match the canonical as_str() value
        if s == "ToolSearch" {
            return Some(ToolName::McpSearch);
        }
        Self::ALL.iter().find(|t| t.as_str() == s).copied()
    }

    /// All builtin tool names.
    pub const ALL: &[ToolName] = &[
        ToolName::Read,
        ToolName::ReadManyFiles,
        ToolName::Glob,
        ToolName::Grep,
        ToolName::Edit,
        ToolName::Write,
        ToolName::Bash,
        ToolName::Task,
        ToolName::TaskOutput,
        ToolName::TaskStop,
        ToolName::TaskCreate,
        ToolName::TaskUpdate,
        ToolName::TaskGet,
        ToolName::TaskList,
        ToolName::TodoWrite,
        ToolName::EnterPlanMode,
        ToolName::ExitPlanMode,
        ToolName::AskUserQuestion,
        ToolName::EnterWorktree,
        ToolName::ExitWorktree,
        ToolName::CronCreate,
        ToolName::CronDelete,
        ToolName::CronList,
        ToolName::TeamCreate,
        ToolName::TeamDelete,
        ToolName::SendMessage,
        ToolName::WebFetch,
        ToolName::WebSearch,
        ToolName::Skill,
        ToolName::LS,
        ToolName::Lsp,
        ToolName::NotebookEdit,
        ToolName::ApplyPatch,
        ToolName::Shell,
        ToolName::SmartEdit,
        ToolName::McpSearch,
    ];
}

impl std::fmt::Display for ToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
