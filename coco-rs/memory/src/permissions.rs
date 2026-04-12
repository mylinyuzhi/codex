//! Extraction agent tool permissions.
//!
//! TS: services/extractMemories/extractMemories.ts — createAutoMemCanUseTool.
//!
//! The extraction agent has restricted tool access:
//! - Read tools (FileRead, Grep, Glob): unrestricted
//! - Bash: read-only commands only
//! - Write tools (FileWrite, FileEdit): only within memory directory
//! - All other tools: denied

use std::path::Path;

/// Tool names used for permission decisions.
///
/// These match the canonical tool names from the tool registry.
pub mod tool_names {
    pub const FILE_READ: &str = "Read";
    pub const FILE_WRITE: &str = "Write";
    pub const FILE_EDIT: &str = "Edit";
    pub const GREP: &str = "Grep";
    pub const GLOB: &str = "Glob";
    pub const BASH: &str = "Bash";
    pub const NOTEBOOK_EDIT: &str = "NotebookEdit";
    pub const AGENT: &str = "Agent";
    pub const MCP_PREFIX: &str = "mcp__";
}

/// Permission decision for a tool call from the extraction agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPermission {
    /// Allow the tool call.
    Allow,
    /// Allow only if the target path is within the memory directory.
    AllowIfMemdir,
    /// Allow only if the command is read-only.
    AllowReadOnly,
    /// Deny the tool call.
    Deny { reason: String },
}

/// Evaluate whether the extraction agent may use a given tool.
///
/// Returns the permission decision based on the tool name.
pub fn evaluate_extraction_tool(tool_name: &str) -> ToolPermission {
    match tool_name {
        // Read tools — unrestricted
        tool_names::FILE_READ | tool_names::GREP | tool_names::GLOB => ToolPermission::Allow,

        // Bash — read-only commands only
        tool_names::BASH => ToolPermission::AllowReadOnly,

        // Write tools — memdir only
        tool_names::FILE_WRITE | tool_names::FILE_EDIT => ToolPermission::AllowIfMemdir,

        // Notebook — deny (no notebooks in memory dir)
        tool_names::NOTEBOOK_EDIT => ToolPermission::Deny {
            reason: "notebook editing not available in extraction mode".to_string(),
        },

        // Agent — deny (no spawning sub-agents from extraction)
        tool_names::AGENT => ToolPermission::Deny {
            reason: "agent spawning not available in extraction mode".to_string(),
        },

        // MCP tools — deny
        name if name.starts_with(tool_names::MCP_PREFIX) => ToolPermission::Deny {
            reason: "MCP tools not available in extraction mode".to_string(),
        },

        // Unknown tools — deny by default
        _ => ToolPermission::Deny {
            reason: format!("tool '{tool_name}' not available in extraction mode"),
        },
    }
}

/// Read-only bash commands allowed for the extraction agent.
///
/// Only these command prefixes are permitted when Bash is used in
/// read-only mode.
pub const READ_ONLY_COMMANDS: &[&str] = &[
    "ls", "find", "grep", "cat", "stat", "wc", "head", "tail", "file", "du",
];

/// Check if a bash command is read-only (safe for extraction agent).
pub fn is_read_only_command(command: &str) -> bool {
    let trimmed = command.trim();
    // Check the first word of the command
    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    READ_ONLY_COMMANDS.contains(&first_word)
}

/// Check if a file path targets the memory directory.
///
/// Used to validate FileWrite/FileEdit calls from the extraction agent.
pub fn is_memdir_path(file_path: &str, memory_dir: &Path) -> bool {
    let path = Path::new(file_path);
    // Handle both absolute and relative paths
    if path.is_absolute() {
        crate::security::is_within_memory_dir(path, memory_dir)
    } else {
        // Relative paths are assumed relative to memory_dir
        let resolved = memory_dir.join(path);
        crate::security::is_within_memory_dir(&resolved, memory_dir)
    }
}

#[cfg(test)]
#[path = "permissions.test.rs"]
mod tests;
