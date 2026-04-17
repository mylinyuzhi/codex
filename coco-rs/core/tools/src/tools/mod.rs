pub mod agent;
pub mod agent_advanced;
pub mod agent_fork;
pub mod agent_handoff;
pub mod agent_resume;
pub mod agent_spawn;
pub mod bash;
pub mod bash_advanced;
pub mod edit;
pub mod edit_utils;
pub mod glob;
pub mod grep;
pub mod mcp_tools;
pub mod plan_worktree;
pub mod powershell;
pub mod powershell_tool;
pub mod read;
pub mod read_permissions;
pub mod scheduling;
pub mod shell_tools;
pub mod task_tools;
pub mod utility;
pub mod web;
pub mod write;

// File I/O (7)
pub use bash::BashTool;
pub use edit::EditTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use read::ReadTool;
pub use utility::NotebookEditTool;
pub use write::WriteTool;

// Web (2)
pub use web::WebFetchTool;
pub use web::WebSearchTool;

// Agent & Team (5)
pub use agent::AgentTool;
pub use agent::SendMessageTool;
pub use agent::SkillTool;
pub use agent::TeamCreateTool;
pub use agent::TeamDeleteTool;

// Fork subagent (B4.1) — re-exports for discoverability. The fork
// infrastructure lives in `agent_fork` but callers (app/query layer)
// need easy access to the top-level guard + context builder when
// wiring the fork path into their AgentHandle implementation.
pub use agent_fork::FORK_BOILERPLATE_TAG;
pub use agent_fork::FORK_DIRECTIVE_PREFIX;
pub use agent_fork::FORK_PLACEHOLDER;
pub use agent_fork::ForkContext;
pub use agent_fork::build_fork_child_message;
pub use agent_fork::build_fork_context;
pub use agent_fork::is_fork_allowed;
pub use agent_fork::is_fork_enabled;
pub use agent_fork::is_in_fork_child;

// Task Management (7)
pub use task_tools::TaskCreateTool;
pub use task_tools::TaskGetTool;
pub use task_tools::TaskListTool;
pub use task_tools::TaskOutputTool;
pub use task_tools::TaskStopTool;
pub use task_tools::TaskUpdateTool;
pub use task_tools::TodoWriteTool;

// Plan & Worktree (4)
pub use plan_worktree::EnterPlanModeTool;
pub use plan_worktree::EnterWorktreeTool;
pub use plan_worktree::ExitPlanModeTool;
pub use plan_worktree::ExitWorktreeTool;

// Utility (5)
pub use utility::AskUserQuestionTool;
pub use utility::BriefTool;
pub use utility::ConfigTool;
pub use utility::LspTool;
pub use utility::ToolSearchTool;

// MCP management (4)
pub use mcp_tools::ListMcpResourcesTool;
pub use mcp_tools::McpAuthTool;
pub use mcp_tools::McpTool;
pub use mcp_tools::ReadMcpResourceTool;

// Scheduling (4)
pub use scheduling::CronCreateTool;
pub use scheduling::CronDeleteTool;
pub use scheduling::CronListTool;
pub use scheduling::RemoteTriggerTool;

// Shell (4)
pub use shell_tools::PowerShellTool;
pub use shell_tools::ReplTool;
pub use shell_tools::SleepTool;
pub use shell_tools::SyntheticOutputTool;

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
