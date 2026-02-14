//! Compile-time embedded prompt templates.
//!
//! Each template is included as a static string from the corresponding `.md` file.

/// Base identity and capabilities.
pub const BASE_IDENTITY: &str = include_str!("base_identity.md");

/// Tool usage rules and policies.
pub const TOOL_POLICY: &str = include_str!("tool_policy.md");

/// Security guidelines.
pub const SECURITY: &str = include_str!("security.md");

/// Git workflow rules.
pub const GIT_WORKFLOW: &str = include_str!("git_workflow.md");

/// Task management approach.
pub const TASK_MANAGEMENT: &str = include_str!("task_management.md");

/// MCP server usage instructions.
pub const MCP_INSTRUCTIONS: &str = include_str!("mcp_instructions.md");

// Note: environment.md is now a Jinja2 template registered in engine.rs,
// no longer exposed as a constant here.

/// Default permission mode instructions.
pub const PERMISSION_DEFAULT: &str = include_str!("permission_default.md");

/// Plan permission mode instructions.
pub const PERMISSION_PLAN: &str = include_str!("permission_plan.md");

/// Accept-edits permission mode instructions.
pub const PERMISSION_ACCEPT_EDITS: &str = include_str!("permission_accept_edits.md");

/// Bypass permission mode instructions.
pub const PERMISSION_BYPASS: &str = include_str!("permission_bypass.md");

/// Explore subagent instructions.
pub const EXPLORE_SUBAGENT: &str = include_str!("explore_subagent.md");

/// Plan subagent instructions.
pub const PLAN_SUBAGENT: &str = include_str!("plan_subagent.md");

/// Summarization template for context compaction.
pub const SUMMARIZATION: &str = include_str!("summarization.md");

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
