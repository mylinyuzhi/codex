//! Compile-time embedded prompt templates.
//!
//! Each template is included as a static string from the corresponding `.md` file.
//!
//! Note: Some templates are now rendered via minijinja (registered in engine.rs):
//! - environment.md
//! - tool_policy_lines.md
//! - memory_files.md
//! - explore_subagent.md
//! - plan_subagent.md
//! - tool_policy.md

/// Base identity and capabilities.
pub const BASE_IDENTITY: &str = include_str!("base_identity.md");

/// Security guidelines.
pub const SECURITY: &str = include_str!("security.md");

/// Git workflow rules.
pub const GIT_WORKFLOW: &str = include_str!("git_workflow.md");

/// Task management approach.
pub const TASK_MANAGEMENT: &str = include_str!("task_management.md");

/// MCP server usage instructions.
pub const MCP_INSTRUCTIONS: &str = include_str!("mcp_instructions.md");

/// Default permission mode instructions.
pub const PERMISSION_DEFAULT: &str = include_str!("permission_default.md");

/// Plan permission mode instructions.
pub const PERMISSION_PLAN: &str = include_str!("permission_plan.md");

/// Accept-edits permission mode instructions.
pub const PERMISSION_ACCEPT_EDITS: &str = include_str!("permission_accept_edits.md");

/// Bypass permission mode instructions.
pub const PERMISSION_BYPASS: &str = include_str!("permission_bypass.md");

/// Summarization template for context compaction.
pub const SUMMARIZATION: &str = include_str!("summarization.md");

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
