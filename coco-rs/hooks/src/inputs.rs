//! Hook input types for all 32 event types.
//!
//! Each event-specific struct embeds `BaseHookInput` via `#[serde(flatten)]`
//! and carries a `hook_event_name` field identifying the event.

use serde::Deserialize;
use serde::Serialize;

use crate::orchestration::OrchestrationContext;

/// Common base fields for all hook inputs.
///
/// TS: createBaseHookInput()
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseHookInput {
    pub session_id: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool-related inputs
// ---------------------------------------------------------------------------

/// Input for PreToolUse hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUseInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,
}

/// Input for PostToolUse hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
    pub tool_use_id: String,
}

/// Input for PostToolUseFailure hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseFailureInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Session lifecycle inputs
// ---------------------------------------------------------------------------

/// Input for SessionStart hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Input for SessionEnd hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEndInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub reason: String,
}

/// Input for Setup hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub trigger: String,
}

/// Input for Stop hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Input for StopFailure hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopFailureInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
}

// ---------------------------------------------------------------------------
// Compact inputs
// ---------------------------------------------------------------------------

/// Input for PreCompact / PostCompact hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactHookInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub trigger: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_summary: Option<String>,
}

// ---------------------------------------------------------------------------
// Subagent inputs
// ---------------------------------------------------------------------------

/// Input for SubagentStart hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentStartInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub agent_type: String,
    pub agent_id: String,
}

/// Input for SubagentStop hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentStopInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub agent_type: String,
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_transcript_path: Option<String>,
}

// ---------------------------------------------------------------------------
// User interaction inputs
// ---------------------------------------------------------------------------

/// Input for UserPromptSubmit hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptSubmitInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub prompt: String,
}

/// Input for PermissionRequest hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,
}

/// Input for PermissionDenied hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDeniedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub tool_name: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Notification / elicitation inputs
// ---------------------------------------------------------------------------

/// Input for Notification hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub notification_type: String,
    pub message: String,
}

/// Input for Elicitation hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub mcp_server_name: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_schema: Option<serde_json::Value>,
}

/// Input for ElicitationResult hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResultInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub mcp_server_name: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// File / config / environment change inputs
// ---------------------------------------------------------------------------

/// Input for FileChanged hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub file_path: String,
    pub event: String,
}

/// Input for ConfigChange hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// Input for InstructionsLoaded hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionsLoadedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub file_path: String,
    pub load_reason: String,
}

/// Input for CwdChanged hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CwdChangedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub old_cwd: String,
    pub new_cwd: String,
}

// ---------------------------------------------------------------------------
// Worktree inputs
// ---------------------------------------------------------------------------

/// Input for WorktreeCreate hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeCreateInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub name: String,
}

/// Input for WorktreeRemove hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRemoveInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub worktree_path: String,
}

// ---------------------------------------------------------------------------
// Task inputs
// ---------------------------------------------------------------------------

/// Input for TaskCreated / TaskCompleted / TeammateIdle hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEventInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Model / resource / query inputs
// ---------------------------------------------------------------------------

/// Input for ModelSwitch hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSwitchInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_model: Option<String>,
    pub to_model: String,
}

/// Input for ContextOverflow / BudgetWarning hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePressureInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    pub pressure_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Input for QueryStart hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryStartInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Input for NotebookCellExecute hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotebookCellExecuteInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub hook_event_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Unified enum
// ---------------------------------------------------------------------------

/// Generic hook input -- wraps any event-specific payload as serialized JSON.
///
/// Used as the unified type for `execute_hooks_parallel`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HookInput {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    PostToolUseFailure(PostToolUseFailureInput),
    SessionStart(SessionStartInput),
    SessionEnd(SessionEndInput),
    Setup(SetupInput),
    Stop(StopInput),
    StopFailure(StopFailureInput),
    Compact(CompactHookInput),
    SubagentStart(SubagentStartInput),
    SubagentStop(SubagentStopInput),
    UserPromptSubmit(UserPromptSubmitInput),
    PermissionRequest(PermissionRequestInput),
    PermissionDenied(PermissionDeniedInput),
    Notification(NotificationInput),
    Elicitation(ElicitationInput),
    ElicitationResult(ElicitationResultInput),
    FileChanged(FileChangedInput),
    ConfigChange(ConfigChangeInput),
    InstructionsLoaded(InstructionsLoadedInput),
    CwdChanged(CwdChangedInput),
    WorktreeCreate(WorktreeCreateInput),
    WorktreeRemove(WorktreeRemoveInput),
    TaskEvent(TaskEventInput),
    ModelSwitch(ModelSwitchInput),
    ResourcePressure(ResourcePressureInput),
    QueryStart(QueryStartInput),
    NotebookCellExecute(NotebookCellExecuteInput),
}

impl HookInput {
    /// The hook event name for this input.
    pub fn event_name(&self) -> &str {
        match self {
            Self::PreToolUse(i) => &i.hook_event_name,
            Self::PostToolUse(i) => &i.hook_event_name,
            Self::PostToolUseFailure(i) => &i.hook_event_name,
            Self::SessionStart(i) => &i.hook_event_name,
            Self::SessionEnd(i) => &i.hook_event_name,
            Self::Setup(i) => &i.hook_event_name,
            Self::Stop(i) => &i.hook_event_name,
            Self::StopFailure(i) => &i.hook_event_name,
            Self::Compact(i) => &i.hook_event_name,
            Self::SubagentStart(i) => &i.hook_event_name,
            Self::SubagentStop(i) => &i.hook_event_name,
            Self::UserPromptSubmit(i) => &i.hook_event_name,
            Self::PermissionRequest(i) => &i.hook_event_name,
            Self::PermissionDenied(i) => &i.hook_event_name,
            Self::Notification(i) => &i.hook_event_name,
            Self::Elicitation(i) => &i.hook_event_name,
            Self::ElicitationResult(i) => &i.hook_event_name,
            Self::FileChanged(i) => &i.hook_event_name,
            Self::ConfigChange(i) => &i.hook_event_name,
            Self::InstructionsLoaded(i) => &i.hook_event_name,
            Self::CwdChanged(i) => &i.hook_event_name,
            Self::WorktreeCreate(i) => &i.hook_event_name,
            Self::WorktreeRemove(i) => &i.hook_event_name,
            Self::TaskEvent(i) => &i.hook_event_name,
            Self::ModelSwitch(i) => &i.hook_event_name,
            Self::ResourcePressure(i) => &i.hook_event_name,
            Self::QueryStart(i) => &i.hook_event_name,
            Self::NotebookCellExecute(i) => &i.hook_event_name,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Build base input from orchestration context.
pub fn base_from_ctx(ctx: &OrchestrationContext) -> BaseHookInput {
    BaseHookInput {
        session_id: ctx.session_id.clone(),
        cwd: ctx.cwd.to_string_lossy().to_string(),
        transcript_path: None,
        permission_mode: ctx.permission_mode.clone(),
    }
}
