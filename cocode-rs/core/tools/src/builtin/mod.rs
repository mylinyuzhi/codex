//! Built-in tools for the agent.
//!
//! This module provides the standard set of built-in tools:
//! - [`ReadTool`] - Read file contents
//! - [`GlobTool`] - Pattern-based file search
//! - [`GrepTool`] - Content search with regex
//! - [`WriteTool`] - Write/edit files
//! - [`BashTool`] - Execute shell commands

mod glob;
mod grep;
mod read;

pub use glob::GlobTool;
pub use grep::GrepTool;
pub use read::ReadTool;

use crate::registry::ToolRegistry;

/// Register all built-in tools with a registry.
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    registry.register(ReadTool::new());
    registry.register(GlobTool::new());
    registry.register(GrepTool::new());
}

/// Get a list of built-in tool names.
pub fn builtin_tool_names() -> Vec<&'static str> {
    vec!["Read", "Glob", "Grep"]
}
