//! Built-in tools for the agent.
//!
//! This module provides the standard set of built-in tools:
//! - [`ReadTool`] - Read file contents
//! - [`ReadManyFilesTool`] - Batch read multiple files
//! - [`GlobTool`] - Pattern-based file search
//! - [`GrepTool`] - Content search with regex
//! - [`EditTool`] - String replacement in files (with flexible whitespace matching)
//! - [`WriteTool`] - Write/create files
//! - [`BashTool`] - Execute shell commands
//! - [`ShellTool`] - Execute commands via array format (direct exec)
//! - [`TaskTool`] - Launch sub-agents
//! - [`TaskOutputTool`] - Get background task output
//! - [`KillShellTool`] - Stop background tasks
//! - [`TodoWriteTool`] - Manage task lists
//! - [`EnterPlanModeTool`] - Enter plan mode
//! - [`ExitPlanModeTool`] - Exit plan mode
//! - [`AskUserQuestionTool`] - Ask interactive questions
//! - [`WebFetchTool`] - Fetch and process web content
//! - [`WebSearchTool`] - Search the web
//! - [`SkillTool`] - Execute named skills (slash commands)
//! - [`LspTool`] - Language Server Protocol operations (feature-gated)
//! - [`McpSearchTool`] - Search MCP tools by keyword (dynamic, for auto-search mode)
//! - [`LsTool`] - List directory contents with tree-style output
//! - [`ApplyPatchTool`] - Apply multi-file patches (optional, for GPT-5)
//! - [`SmartEditTool`] - Edit with LLM correction fallback (feature-gated)
//! - [`TaskCreateTool`] - Create structured tasks with dependencies (feature-gated)
//! - [`TaskUpdateTool`] - Update structured task status and metadata (feature-gated)
//! - [`TaskGetTool`] - Retrieve a single structured task (feature-gated)
//! - [`TaskListTool`] - List structured tasks with filtering (feature-gated)
//! - [`EnterWorktreeTool`] - Create isolated git worktrees (feature-gated)
//! - [`ExitWorktreeTool`] - Exit and clean up git worktrees (feature-gated)
//! - [`CronCreateTool`] - Schedule recurring cron jobs (feature-gated)
//! - [`CronDeleteTool`] - Delete scheduled cron jobs (feature-gated)
//! - [`CronListTool`] - List active cron jobs (feature-gated)
//! - [`TeamCreateTool`] - Create named agent teams (feature-gated)
//! - [`TeamDeleteTool`] - Delete agent teams (feature-gated)
//! - [`SendMessageTool`] - Inter-agent messaging (feature-gated)
//!
//! ## Utilities
//!
//! - [`path_extraction::LlmPathExtractor`] - LLM-based file path extraction from command output

mod prompts;

pub(crate) mod input_helpers;

/// Map a shell-parser security risk to a protocol-level [`SecurityRisk`].
///
/// Uses exhaustive matching on [`RiskKind`] per CLAUDE.md guidelines.
fn map_shell_risk(
    r: &cocode_shell_parser::security::SecurityRisk,
) -> cocode_protocol::SecurityRisk {
    use cocode_shell_parser::security::RiskKind;
    use cocode_shell_parser::security::RiskLevel;

    let risk_type = match r.kind {
        // Ask-phase risks with specific mappings
        RiskKind::NetworkExfiltration => cocode_protocol::RiskType::Network,
        RiskKind::PrivilegeEscalation => cocode_protocol::RiskType::Elevated,
        RiskKind::FileSystemTampering => cocode_protocol::RiskType::Destructive,
        RiskKind::SensitiveRedirect => cocode_protocol::RiskType::SensitiveFile,
        RiskKind::CodeExecution
        | RiskKind::UnsafeHeredocSubstitution
        | RiskKind::DangerousSubstitution => cocode_protocol::RiskType::SystemConfig,
        RiskKind::MalformedTokens => cocode_protocol::RiskType::Unknown,
        // Deny-phase risks should not reach ask-phase mapping (they are
        // auto-denied earlier), but we enumerate them explicitly to satisfy
        // exhaustive-match rules and avoid silent misclassification.
        RiskKind::SingleQuoteBypass
        | RiskKind::JqDanger
        | RiskKind::ObfuscatedFlags
        | RiskKind::ShellMetacharacters
        | RiskKind::DangerousVariables
        | RiskKind::NewlineInjection
        | RiskKind::IfsInjection
        | RiskKind::ProcEnvironAccess
        | RiskKind::BackslashEscapedWhitespace
        | RiskKind::BackslashEscapedOperators
        | RiskKind::UnicodeWhitespace
        | RiskKind::MidWordHash
        | RiskKind::BraceExpansion
        | RiskKind::ZshDangerousCommands
        | RiskKind::CommentQuoteDesync
        | RiskKind::QuotedNewlineHash => cocode_protocol::RiskType::Unknown,
    };

    let severity = match r.level {
        RiskLevel::Low => cocode_protocol::RiskSeverity::Low,
        RiskLevel::Medium => cocode_protocol::RiskSeverity::Medium,
        RiskLevel::High => cocode_protocol::RiskSeverity::High,
        RiskLevel::Critical => cocode_protocol::RiskSeverity::Critical,
    };

    cocode_protocol::SecurityRisk {
        risk_type,
        severity,
        message: r.message.clone(),
    }
}

/// Shared output formatting: redact secrets and wrap in [`ToolOutput`].
///
/// Used by both [`BashTool`] and [`ShellTool`] to consistently format
/// command output with secret redaction.
fn format_redacted_output(
    text: &str,
    exit_code: i32,
) -> crate::error::Result<cocode_protocol::ToolOutput> {
    let text = cocode_secret_redact::redact_secrets(text);

    if exit_code != 0 {
        return if text.is_empty() {
            Ok(cocode_protocol::ToolOutput::error(format!(
                "Command failed with exit code {exit_code}"
            )))
        } else {
            Ok(cocode_protocol::ToolOutput::error(format!(
                "{text}\n\nExit code: {exit_code}"
            )))
        };
    }

    if text.is_empty() {
        return Ok(cocode_protocol::ToolOutput::text("(no output)"));
    }
    Ok(cocode_protocol::ToolOutput::text(text.into_owned()))
}

mod apply_patch;
mod ask_user_question;
mod bash;
mod cron_create;
mod cron_delete;
mod cron_list;
mod edit;
mod edit_strategies;
mod enter_plan_mode;
mod enter_worktree;
mod exit_plan_mode;
mod exit_worktree;
mod glob;
mod grep;
mod kill_shell;
mod ls;
mod lsp;
pub mod mcp_search;
mod notebook_edit;
pub mod path_extraction;
mod read;
mod read_many;
mod send_message;
mod shell;
mod skill;
mod smart_edit;
pub mod structured_tasks;
mod task;
mod task_create;
mod task_get;
mod task_list;
mod task_output;
mod task_update;
mod team_create;
mod team_delete;
pub mod team_state;
mod todo_write;
mod web_fetch;
mod web_search;
mod write;

pub use apply_patch::ApplyPatchTool;
pub use ask_user_question::AskUserQuestionTool;
pub use bash::BashTool;
pub use cron_create::CronCreateTool;
pub use cron_delete::CronDeleteTool;
pub use cron_list::CronListTool;
pub use edit::EditTool;
pub use enter_plan_mode::EnterPlanModeTool;
pub use enter_worktree::EnterWorktreeTool;
pub use exit_plan_mode::ExitPlanModeTool;
pub use exit_worktree::ExitWorktreeTool;
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use kill_shell::KillShellTool;
pub use ls::LsTool;
pub use lsp::LspTool;
pub use mcp_search::McpSearchTool;
pub use notebook_edit::NotebookEditTool;
pub use read::ReadTool;
pub use read_many::ReadManyFilesTool;
pub use send_message::SendMessageTool;
pub use shell::ShellTool;
pub use skill::SkillTool;
pub use smart_edit::SmartEditTool;
pub use task::TaskTool;
pub use task_create::TaskCreateTool;
pub use task_get::TaskGetTool;
pub use task_list::TaskListTool;
pub use task_output::TaskOutputTool;
pub use task_update::TaskUpdateTool;
pub use team_create::TeamCreateTool;
pub use team_delete::TeamDeleteTool;
pub use todo_write::TodoWriteTool;
pub use web_fetch::WebFetchTool;
pub use web_search::WebSearchTool;
pub use write::WriteTool;

use crate::registry::ToolRegistry;
use cocode_protocol::Features;

/// Shared stores returned by `register_builtin_tools()`.
///
/// Allows the caller (e.g. `SessionState`) to access shared state
/// that tools were constructed with.
pub struct BuiltinStores {
    /// The shared cron job store used by CronCreate/CronDelete/CronList.
    pub cron_store: cocode_cron::CronJobStore,
    /// The shared team store used by TeamCreate/TeamDelete/SendMessage.
    pub team_store: std::sync::Arc<cocode_team::TeamStore>,
    /// The shared mailbox used by SendMessage.
    pub mailbox: std::sync::Arc<cocode_team::Mailbox>,
}

/// Create default team stores for use when no pre-loaded stores are available.
///
/// The returned stores are **not** loaded from disk. If persistence is needed,
/// call `team_store.load_from_disk().await` before registering tools.
pub fn create_default_team_stores() -> (
    std::sync::Arc<cocode_team::TeamStore>,
    std::sync::Arc<cocode_team::Mailbox>,
) {
    let team_base_dir = cocode_config::find_cocode_home().join("teams");
    let team_store = std::sync::Arc::new(cocode_team::TeamStore::new(team_base_dir.clone(), true));
    let mailbox = std::sync::Arc::new(cocode_team::Mailbox::new(team_base_dir));
    (team_store, mailbox)
}

/// Register all built-in tools with a registry.
///
/// All tools including `apply_patch` are always registered. Which tool
/// definitions are sent to a model is decided at request time by
/// `select_tools_for_model()` based on `ModelInfo.apply_patch_tool_type`.
///
/// The `features` parameter is used to configure interview-conditional
/// tool descriptions (e.g., EnterPlanMode).
///
/// `team_store` and `mailbox` should be pre-loaded (via `TeamStore::load_from_disk()`)
/// before calling this function. Use [`create_default_team_stores`] if no
/// pre-loaded stores are available.
///
/// Returns [`BuiltinStores`] containing shared state handles for the
/// registered tools (e.g. the cron job store for durable persistence).
pub fn register_builtin_tools(
    registry: &mut ToolRegistry,
    features: &Features,
    team_store: std::sync::Arc<cocode_team::TeamStore>,
    mailbox: std::sync::Arc<cocode_team::Mailbox>,
) -> BuiltinStores {
    let interview_phase = features.enabled(cocode_protocol::Feature::PlanModeInterview);

    registry.register(ReadTool::new());
    registry.register(GlobTool::new());
    registry.register(GrepTool::new());
    registry.register(EditTool::new());
    registry.register(WriteTool::new());
    registry.register(BashTool::new());
    registry.register(TaskTool::new());
    registry.register(TaskOutputTool::new());
    registry.register(KillShellTool::new());
    registry.register(TodoWriteTool::new());
    registry.register(EnterPlanModeTool::with_interview_phase(interview_phase));
    registry.register(ExitPlanModeTool::new());
    registry.register(AskUserQuestionTool::new());
    registry.register(WebFetchTool::new());
    registry.register(WebSearchTool::new());
    registry.register(SkillTool::new());
    registry.register(LsTool::new());
    registry.register(LspTool::new());
    registry.register(NotebookEditTool::new());
    registry.register(ApplyPatchTool::new());
    registry.register(ShellTool::new());
    registry.register(ReadManyFilesTool::new());
    registry.register(SmartEditTool::new());
    registry.register(EnterWorktreeTool::new());
    registry.register(ExitWorktreeTool::new());

    // Structured task management tools (shared store)
    let task_store = structured_tasks::new_task_store();
    registry.register(TaskCreateTool::new(task_store.clone()));
    registry.register(TaskUpdateTool::new(task_store.clone()));
    registry.register(TaskGetTool::new(task_store.clone()));
    registry.register(TaskListTool::new(task_store));

    // Cron scheduling tools (shared store)
    let cron_store = cocode_cron::new_cron_store();
    registry.register(CronCreateTool::new(cron_store.clone()));
    registry.register(CronDeleteTool::new(cron_store.clone()));
    registry.register(CronListTool::new(cron_store.clone()));

    // Team/collaboration tools (pre-loaded team store + mailbox)
    registry.register(TeamCreateTool::new(team_store.clone()));
    registry.register(TeamDeleteTool::new(team_store.clone()));
    registry.register(SendMessageTool::new(team_store.clone(), mailbox.clone()));

    BuiltinStores {
        cron_store,
        team_store,
        mailbox,
    }
}

/// Get a list of built-in tool names.
#[allow(clippy::redundant_closure_for_method_calls)]
pub fn builtin_tool_names() -> Vec<&'static str> {
    cocode_protocol::ToolName::ALL
        .iter()
        .map(|t| t.as_str())
        .collect()
}
