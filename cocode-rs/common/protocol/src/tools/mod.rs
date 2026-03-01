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

    // Planning and interaction
    EnterPlanMode,
    ExitPlanMode,
    AskUserQuestion,
    TodoWrite,

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
            ToolName::EnterPlanMode => "EnterPlanMode",
            ToolName::ExitPlanMode => "ExitPlanMode",
            ToolName::AskUserQuestion => "AskUserQuestion",
            ToolName::TodoWrite => "TodoWrite",
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
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Read" => Some(ToolName::Read),
            "ReadManyFiles" => Some(ToolName::ReadManyFiles),
            "Glob" => Some(ToolName::Glob),
            "Grep" => Some(ToolName::Grep),
            "Edit" => Some(ToolName::Edit),
            "Write" => Some(ToolName::Write),
            "LS" => Some(ToolName::LS),
            "Bash" => Some(ToolName::Bash),
            "shell" => Some(ToolName::Shell),
            "Task" => Some(ToolName::Task),
            "TaskOutput" => Some(ToolName::TaskOutput),
            "TaskStop" => Some(ToolName::TaskStop),
            "EnterPlanMode" => Some(ToolName::EnterPlanMode),
            "ExitPlanMode" => Some(ToolName::ExitPlanMode),
            "AskUserQuestion" => Some(ToolName::AskUserQuestion),
            "TodoWrite" => Some(ToolName::TodoWrite),
            "WebFetch" => Some(ToolName::WebFetch),
            "WebSearch" => Some(ToolName::WebSearch),
            "Skill" => Some(ToolName::Skill),
            "Lsp" => Some(ToolName::Lsp),
            "NotebookEdit" => Some(ToolName::NotebookEdit),
            "SmartEdit" => Some(ToolName::SmartEdit),
            "apply_patch" => Some(ToolName::ApplyPatch),
            "MCPSearch" => Some(ToolName::McpSearch),
            _ => None,
        }
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
        ToolName::TodoWrite,
        ToolName::EnterPlanMode,
        ToolName::ExitPlanMode,
        ToolName::AskUserQuestion,
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
mod tests {
    use super::*;

    #[test]
    fn test_as_str_roundtrip() {
        for tool in ToolName::ALL {
            let s = tool.as_str();
            let parsed = ToolName::from_str(s);
            assert_eq!(parsed, Some(*tool), "Failed to roundtrip {:?}", tool);
        }
    }

    #[test]
    fn test_unknown_returns_none() {
        assert_eq!(ToolName::from_str("unknown_tool"), None);
        assert_eq!(ToolName::from_str("mcp_some_tool"), None);
    }

    #[test]
    fn test_special_names() {
        // Shell is lowercase
        assert_eq!(ToolName::Shell.as_str(), "shell");
        assert_eq!(ToolName::from_str("shell"), Some(ToolName::Shell));

        // ApplyPatch is lowercase with underscore
        assert_eq!(ToolName::ApplyPatch.as_str(), "apply_patch");
        assert_eq!(
            ToolName::from_str("apply_patch"),
            Some(ToolName::ApplyPatch)
        );

        // MCPSearch has uppercase MCP
        assert_eq!(ToolName::McpSearch.as_str(), "MCPSearch");
        assert_eq!(ToolName::from_str("MCPSearch"), Some(ToolName::McpSearch));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", ToolName::Read), "Read");
        assert_eq!(format!("{}", ToolName::Shell), "shell");
        assert_eq!(format!("{}", ToolName::ApplyPatch), "apply_patch");
    }

    #[test]
    fn test_serde() {
        let json = serde_json::to_string(&ToolName::Read).unwrap();
        assert_eq!(json, "\"Read\"");

        let parsed: ToolName = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ToolName::Read);
    }

    #[test]
    fn test_all_count() {
        // Ensure ALL contains all variants (24 tools)
        assert_eq!(ToolName::ALL.len(), 24);
    }
}
