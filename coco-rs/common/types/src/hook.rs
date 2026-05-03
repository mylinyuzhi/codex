use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::IntoStaticStr;

/// 32 hook event types (synced with TS coreSchemas.ts HOOK_EVENTS).
/// Uses #[non_exhaustive] because TS adds new events across versions.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[non_exhaustive]
pub enum HookEventType {
    // Tool lifecycle
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    // Session lifecycle
    SessionStart,
    SessionEnd,
    Setup,
    Stop,
    StopFailure,
    // Subagent lifecycle
    SubagentStart,
    SubagentStop,
    // User interaction
    UserPromptSubmit,
    PermissionRequest,
    PermissionDenied,
    Notification,
    Elicitation,
    ElicitationResult,
    // Compaction
    PreCompact,
    PostCompact,
    // Task lifecycle
    TeammateIdle,
    TaskCreated,
    TaskCompleted,
    // Config & environment
    ConfigChange,
    InstructionsLoaded,
    CwdChanged,
    FileChanged,
    // Worktree
    WorktreeCreate,
    WorktreeRemove,
    // Notebook
    NotebookCellExecute,
    // Model
    ModelSwitch,
    // Resource pressure
    ContextOverflow,
    BudgetWarning,
    // Query
    QueryStart,
}

/// Scope that determines hook priority ordering.
///
/// Higher-priority scopes override lower ones. Ordering matches the TS
/// implementation: Session (most specific) > Local > Project > User > Plugin/Builtin.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum HookScope {
    /// Builtin/plugin hooks (lowest priority).
    Builtin = 0,
    /// User-level hooks from ~/.config settings.
    #[default]
    User = 1,
    /// Project-level hooks from .claude/ settings.
    Project = 2,
    /// Local (machine-specific) overrides.
    Local = 3,
    /// Session-specific hooks (highest priority).
    Session = 4,
}

/// Outcome of hook execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    Success,
    Blocking,
    NonBlockingError,
    Cancelled,
}

// `HookResult` (which embeds `Option<Message>`) lives in `coco-messages`;
// hook protocol enums (HookEventType / HookOutcome / HookScope) stay here.
