//! Tool name constants for the cocode ecosystem.
//!
//! All tool names are defined here as constants to ensure consistency
//! across the codebase. When referencing a tool name, use these constants
//! instead of hardcoded string literals.

/// File operations
pub const READ: &str = "Read";
pub const READ_MANY_FILES: &str = "ReadManyFiles";
pub const GLOB: &str = "Glob";
pub const GREP: &str = "Grep";
pub const EDIT: &str = "Edit";
pub const WRITE: &str = "Write";
pub const LS: &str = "LS";

/// Shell/Command execution
pub const BASH: &str = "Bash";
pub const SHELL: &str = "shell";

/// Task management
pub const TASK: &str = "Task";
pub const TASK_OUTPUT: &str = "TaskOutput";
pub const TASK_STOP: &str = "TaskStop";

/// Planning and interaction
pub const ENTER_PLAN_MODE: &str = "EnterPlanMode";
pub const EXIT_PLAN_MODE: &str = "ExitPlanMode";
pub const ASK_USER_QUESTION: &str = "AskUserQuestion";
pub const TODO_WRITE: &str = "TodoWrite";

/// Web tools
pub const WEB_FETCH: &str = "WebFetch";
pub const WEB_SEARCH: &str = "WebSearch";

/// Language/Skill tools
pub const SKILL: &str = "Skill";
pub const LSP: &str = "Lsp";
pub const NOTEBOOK_EDIT: &str = "NotebookEdit";
pub const SMART_EDIT: &str = "SmartEdit";

/// Patch tool (lowercase for OpenAI compatibility)
pub const APPLY_PATCH: &str = "apply_patch";

/// MCP tools
pub const MCP_SEARCH: &str = "MCPSearch";

/// All builtin tool names as a slice.
pub const BUILTIN_TOOL_NAMES: &[&str] = &[
    READ,
    READ_MANY_FILES,
    GLOB,
    GREP,
    EDIT,
    WRITE,
    BASH,
    TASK,
    TASK_OUTPUT,
    TASK_STOP,
    TODO_WRITE,
    ENTER_PLAN_MODE,
    EXIT_PLAN_MODE,
    ASK_USER_QUESTION,
    WEB_FETCH,
    WEB_SEARCH,
    SKILL,
    LS,
    LSP,
    NOTEBOOK_EDIT,
    APPLY_PATCH,
    SHELL,
    SMART_EDIT,
];