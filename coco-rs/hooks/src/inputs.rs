//! Hook input types for all 27 TS-canonical event types.
//!
//! Each event-specific struct embeds `BaseHookInput` via `#[serde(flatten)]`.
//! The TS-wire `hook_event_name` discriminator is supplied by the
//! [`HookInput`] enum's `#[serde(tag = "hook_event_name")]` representation
//! — it is not a Rust field on the inner structs (one source of truth).
//!
//! Field shapes mirror `entrypoints/sdk/coreSchemas.ts` from claude-code TS.

use coco_types::HookEventType;
use serde::Deserialize;
use serde::Serialize;

use crate::orchestration::OrchestrationContext;

// ---------------------------------------------------------------------------
// Enum-typed fields (TS zod enums)
// ---------------------------------------------------------------------------

/// SessionStart `source`. TS: `enum(['startup','resume','clear','compact'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStartSource {
    Startup,
    Resume,
    Clear,
    Compact,
}

/// Setup `trigger`. TS: `enum(['init','maintenance'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupTrigger {
    Init,
    Maintenance,
}

/// Pre/PostCompact `trigger`. TS: `enum(['manual','auto'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTrigger {
    Manual,
    Auto,
}

/// SessionEnd `reason`. TS:
/// `enum(['clear','resume','logout','prompt_input_exit','other','bypass_permissions_disabled'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    Clear,
    Resume,
    Logout,
    PromptInputExit,
    Other,
    BypassPermissionsDisabled,
}

/// FileChanged `event`. TS: `enum(['change','add','unlink'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeEvent {
    Change,
    Add,
    Unlink,
}

/// ConfigChange `source`. TS:
/// `enum(['user_settings','project_settings','local_settings','policy_settings','skills'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigChangeSource {
    UserSettings,
    ProjectSettings,
    LocalSettings,
    PolicySettings,
    Skills,
}

/// InstructionsLoaded `memory_type`. TS:
/// `enum(['User','Project','Local','Managed'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryType {
    User,
    Project,
    Local,
    Managed,
}

/// InstructionsLoaded `load_reason`. TS:
/// `enum(['session_start','nested_traversal','path_glob_match','include','compact'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionsLoadReason {
    SessionStart,
    NestedTraversal,
    PathGlobMatch,
    Include,
    Compact,
}

/// Elicitation `mode`. TS: `enum(['form','url'])`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElicitationMode {
    Form,
    Url,
}

/// ElicitationResult `action`. TS: `enum(['accept','decline','cancel'])`.
///
/// Re-exported from `coco_types` so the hook **input** type
/// (`ElicitationResultInput.action`) and the hook **output** type
/// (`HookSpecificOutput::ElicitationResult.action`) reference the same
/// enum — one wire vocabulary, single source of truth.
pub use coco_types::ElicitationAction;

impl SessionStartSource {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Resume => "resume",
            Self::Clear => "clear",
            Self::Compact => "compact",
        }
    }
}

impl SetupTrigger {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Maintenance => "maintenance",
        }
    }
}

impl CompactTrigger {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

impl ExitReason {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Resume => "resume",
            Self::Logout => "logout",
            Self::PromptInputExit => "prompt_input_exit",
            Self::Other => "other",
            Self::BypassPermissionsDisabled => "bypass_permissions_disabled",
        }
    }
}

impl FileChangeEvent {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Change => "change",
            Self::Add => "add",
            Self::Unlink => "unlink",
        }
    }
}

impl ConfigChangeSource {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::UserSettings => "user_settings",
            Self::ProjectSettings => "project_settings",
            Self::LocalSettings => "local_settings",
            Self::PolicySettings => "policy_settings",
            Self::Skills => "skills",
        }
    }
}

impl MemoryType {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Project => "Project",
            Self::Local => "Local",
            Self::Managed => "Managed",
        }
    }
}

impl InstructionsLoadReason {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::NestedTraversal => "nested_traversal",
            Self::PathGlobMatch => "path_glob_match",
            Self::Include => "include",
            Self::Compact => "compact",
        }
    }
}

impl ElicitationMode {
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::Form => "form",
            Self::Url => "url",
        }
    }
}

/// Wire-format string for an [`ElicitationAction`]. Free function
/// because `ElicitationAction` lives in `coco-types` and we can't add
/// an inherent impl from this crate.
pub fn elicitation_action_wire_str(action: ElicitationAction) -> &'static str {
    match action {
        ElicitationAction::Accept => "accept",
        ElicitationAction::Decline => "decline",
        ElicitationAction::Cancel => "cancel",
    }
}

/// Common base fields for all hook inputs.
///
/// TS: `BaseHookInputSchema` (`coreSchemas.ts:387-411`). All four base
/// fields (`session_id`, `transcript_path`, `cwd`) are required; in
/// practice TS callers default `transcript_path` to an empty string
/// when no transcript file is being persisted, so we mirror that here
/// (the field is `String`, defaulting to `""` via `base_from_ctx`).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaseHookInput {
    pub session_id: String,
    pub cwd: String,
    /// Path to the on-disk transcript file. Empty string when the
    /// session is not persisting a transcript (matches TS practice of
    /// always emitting a string value). Defaults to `""` on
    /// deserialize so older fixtures missing the field still parse.
    #[serde(default)]
    pub transcript_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// Subagent identifier — present only when the hook fires from
    /// within a subagent (e.g. a tool called by an `AgentTool` worker).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Subagent type (e.g. `"Explore"`, `"Review"`) — set on subagent
    /// hooks AND on main-thread hooks when the session was launched
    /// via `--agent` (TS parity).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool-related inputs
// ---------------------------------------------------------------------------

/// Input for PreToolUse hooks. TS: `PreToolUseHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUseInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,
}

/// Input for PostToolUse hooks. TS: `PostToolUseHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
    pub tool_use_id: String,
}

/// Input for PostToolUseFailure hooks.
///
/// TS: `PostToolUseFailureHookInputSchema` (`coreSchemas.ts:448-459`):
/// `{tool_name, tool_input, tool_use_id, error, is_interrupt?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseFailureInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,
    pub error: String,
    /// `true` when the tool call was aborted because the user
    /// interrupted the turn (TS `processInterrupt` path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_interrupt: Option<bool>,
}

// ---------------------------------------------------------------------------
// Session lifecycle inputs
// ---------------------------------------------------------------------------

/// Input for SessionStart hooks. TS: `SessionStartHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStartInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub source: SessionStartSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Input for SessionEnd hooks. TS: `SessionEndHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEndInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub reason: ExitReason,
}

/// Input for Setup hooks. TS: `SetupHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub trigger: SetupTrigger,
}

/// Input for Stop hooks.
///
/// TS: `StopHookInputSchema` (`coreSchemas.ts:513-527`):
/// `{stop_hook_active, last_assistant_message?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    /// `true` when the Stop hook is firing recursively (a previous
    /// Stop hook blocked, the loop continued, and Stop is firing
    /// again). Hooks should typically pass through to avoid infinite
    /// loops.
    pub stop_hook_active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
}

/// Input for StopFailure hooks. TS: `StopFailureHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopFailureInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
}

// ---------------------------------------------------------------------------
// Compact inputs
// ---------------------------------------------------------------------------

/// Input for PreCompact hooks.
///
/// TS: `PreCompactHookInputSchema` (`coreSchemas.ts:569-577`):
/// `{trigger: enum('manual','auto'), custom_instructions: string | null}`.
/// `custom_instructions` is **nullable, not optional** — the field is
/// always present on the wire, with `null` indicating no instructions.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreCompactInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub trigger: CompactTrigger,
    /// `None` serializes to JSON `null` (TS-nullable parity); the
    /// field is intentionally NOT skip_serializing_if so it always
    /// appears on the wire.
    pub custom_instructions: Option<String>,
}

/// Input for PostCompact hooks.
///
/// TS: `PostCompactHookInputSchema` (`coreSchemas.ts:579-589`):
/// `{trigger: enum('manual','auto'), compact_summary: string}`. Both
/// fields are required.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostCompactInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub trigger: CompactTrigger,
    pub compact_summary: String,
}

// ---------------------------------------------------------------------------
// Subagent inputs
// ---------------------------------------------------------------------------

/// Input for SubagentStart hooks. TS: `SubagentStartHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentStartInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub agent_type: String,
    pub agent_id: String,
}

/// Input for SubagentStop hooks.
///
/// TS: `SubagentStopHookInputSchema` (`coreSchemas.ts:550-567`):
/// `{stop_hook_active, agent_id, agent_transcript_path, agent_type, last_assistant_message?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentStopInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub stop_hook_active: bool,
    pub agent_type: String,
    pub agent_id: String,
    /// Path to the subagent's transcript file. Empty string when the
    /// subagent is not persisting one (TS marks this required).
    pub agent_transcript_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
}

// ---------------------------------------------------------------------------
// User interaction inputs
// ---------------------------------------------------------------------------

/// Input for UserPromptSubmit hooks. TS: `UserPromptSubmitHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPromptSubmitInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub prompt: String,
}

/// Input for PermissionRequest hooks.
///
/// TS: `PermissionRequestHookInputSchema` (`coreSchemas.ts:425-434`):
/// `{tool_name, tool_input, permission_suggestions?}` — note that TS
/// does NOT include `tool_use_id` on this event.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequestInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    /// Suggested permission updates from upstream classifiers. JSON
    /// pass-through to match TS `PermissionUpdate[]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_suggestions: Option<serde_json::Value>,
}

/// Input for PermissionDenied hooks.
///
/// TS: `PermissionDeniedHookInputSchema` (`coreSchemas.ts:461-471`):
/// `{tool_name, tool_input, tool_use_id, reason}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDeniedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_use_id: String,
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Notification / elicitation inputs
// ---------------------------------------------------------------------------

/// Input for Notification hooks.
///
/// TS: `NotificationHookInputSchema` (`coreSchemas.ts:473-482`):
/// `{message, title?, notification_type}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub notification_type: String,
}

/// Input for Elicitation hooks.
///
/// TS: `ElicitationHookInputSchema` (`coreSchemas.ts:627-643`):
/// `{mcp_server_name, message, mode?, url?, elicitation_id?, requested_schema?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub mcp_server_name: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ElicitationMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_schema: Option<serde_json::Value>,
}

/// Input for ElicitationResult hooks.
///
/// TS: `ElicitationResultHookInputSchema` (`coreSchemas.ts:645-660`):
/// `{mcp_server_name, elicitation_id?, mode?, action, content?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElicitationResultInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub mcp_server_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ElicitationMode>,
    pub action: ElicitationAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// File / config / environment change inputs
// ---------------------------------------------------------------------------

/// Input for FileChanged hooks. TS: `FileChangedHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChangedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub file_path: String,
    pub event: FileChangeEvent,
}

/// Input for ConfigChange hooks. TS: `ConfigChangeHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub source: ConfigChangeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// Input for InstructionsLoaded hooks.
///
/// TS: `InstructionsLoadedHookInputSchema` (`coreSchemas.ts:695-706`):
/// `{file_path, memory_type, load_reason, globs?, trigger_file_path?, parent_file_path?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionsLoadedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub file_path: String,
    pub memory_type: MemoryType,
    pub load_reason: InstructionsLoadReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub globs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_file_path: Option<String>,
}

/// Input for CwdChanged hooks. TS: `CwdChangedHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CwdChangedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub old_cwd: String,
    pub new_cwd: String,
}

// ---------------------------------------------------------------------------
// Worktree inputs
// ---------------------------------------------------------------------------

/// Input for WorktreeCreate hooks. TS: `WorktreeCreateHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeCreateInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub name: String,
}

/// Input for WorktreeRemove hooks. TS: `WorktreeRemoveHookInputSchema`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeRemoveInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub worktree_path: String,
}

// ---------------------------------------------------------------------------
// Task inputs
// ---------------------------------------------------------------------------

/// Input for TaskCreated hooks.
///
/// TS: `TaskCreatedHookInputSchema` (`coreSchemas.ts:601-612`):
/// `{task_id, task_subject, task_description?, teammate_name?, team_name?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCreatedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub task_id: String,
    pub task_subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teammate_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
}

/// Input for TaskCompleted hooks.
///
/// TS: `TaskCompletedHookInputSchema` (`coreSchemas.ts:614-625`):
/// `{task_id, task_subject, task_description?, teammate_name?, team_name?}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCompletedInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub task_id: String,
    pub task_subject: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub teammate_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
}

/// Input for TeammateIdle hooks.
///
/// TS: `TeammateIdleHookInputSchema` (`coreSchemas.ts:591-599`):
/// `{teammate_name, team_name}`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateIdleInput {
    #[serde(flatten)]
    pub base: BaseHookInput,
    pub teammate_name: String,
    pub team_name: String,
}

// ---------------------------------------------------------------------------
// Unified enum
// ---------------------------------------------------------------------------

/// Generic hook input — unified envelope for every TS hook event.
///
/// Internally tagged on `hook_event_name` (PascalCase wire literal,
/// matching `HookEventType`). The tag field is supplied by serde from
/// the variant identity, so inner structs do NOT carry a redundant
/// `hook_event_name` field. Wire shape is identical to the prior
/// `untagged` representation:
///
/// ```json
/// {"hook_event_name":"PreToolUse","session_id":"s","tool_name":"Read",...}
/// ```
///
/// Compared with `untagged`, this representation:
///  - lets schemars emit a discriminated `oneOf` with `const` on the
///    tag field, which downstream codegen (Pydantic discriminated
///    unions, TS native discriminated-union narrowing) consumes natively;
///  - replaces serde's try-each-variant deserialize loop with O(1)
///    dispatch on the tag value.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "hook_event_name")]
pub enum HookInput {
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    PostToolUseFailure(PostToolUseFailureInput),
    SessionStart(SessionStartInput),
    SessionEnd(SessionEndInput),
    Setup(SetupInput),
    Stop(StopInput),
    StopFailure(StopFailureInput),
    PreCompact(PreCompactInput),
    PostCompact(PostCompactInput),
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
    TaskCreated(TaskCreatedInput),
    TaskCompleted(TaskCompletedInput),
    TeammateIdle(TeammateIdleInput),
}

impl HookInput {
    /// The hook event for this input.
    ///
    /// The variant identity *is* the event — there is no separate
    /// field to read. The wire-format `hook_event_name` is emitted by
    /// serde from the tag attribute, so this match is the single
    /// source of truth for the in-memory event.
    pub fn event(&self) -> HookEventType {
        match self {
            Self::PreToolUse(_) => HookEventType::PreToolUse,
            Self::PostToolUse(_) => HookEventType::PostToolUse,
            Self::PostToolUseFailure(_) => HookEventType::PostToolUseFailure,
            Self::SessionStart(_) => HookEventType::SessionStart,
            Self::SessionEnd(_) => HookEventType::SessionEnd,
            Self::Setup(_) => HookEventType::Setup,
            Self::Stop(_) => HookEventType::Stop,
            Self::StopFailure(_) => HookEventType::StopFailure,
            Self::PreCompact(_) => HookEventType::PreCompact,
            Self::PostCompact(_) => HookEventType::PostCompact,
            Self::SubagentStart(_) => HookEventType::SubagentStart,
            Self::SubagentStop(_) => HookEventType::SubagentStop,
            Self::UserPromptSubmit(_) => HookEventType::UserPromptSubmit,
            Self::PermissionRequest(_) => HookEventType::PermissionRequest,
            Self::PermissionDenied(_) => HookEventType::PermissionDenied,
            Self::Notification(_) => HookEventType::Notification,
            Self::Elicitation(_) => HookEventType::Elicitation,
            Self::ElicitationResult(_) => HookEventType::ElicitationResult,
            Self::FileChanged(_) => HookEventType::FileChanged,
            Self::ConfigChange(_) => HookEventType::ConfigChange,
            Self::InstructionsLoaded(_) => HookEventType::InstructionsLoaded,
            Self::CwdChanged(_) => HookEventType::CwdChanged,
            Self::WorktreeCreate(_) => HookEventType::WorktreeCreate,
            Self::WorktreeRemove(_) => HookEventType::WorktreeRemove,
            Self::TaskCreated(_) => HookEventType::TaskCreated,
            Self::TaskCompleted(_) => HookEventType::TaskCompleted,
            Self::TeammateIdle(_) => HookEventType::TeammateIdle,
        }
    }

    /// The hook event name (TS wire-format string) for this input.
    pub fn event_name(&self) -> &'static str {
        self.event().as_str()
    }
}

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

/// Build base input from orchestration context.
///
/// `transcript_path` defaults to `""` when the context does not carry
/// one — TS marks the field required, and emitting an empty string
/// rather than `null` matches its serde shape.
pub fn base_from_ctx(ctx: &OrchestrationContext) -> BaseHookInput {
    BaseHookInput {
        session_id: ctx.session_id.clone(),
        cwd: ctx.cwd.to_string_lossy().to_string(),
        transcript_path: ctx.transcript_path.clone().unwrap_or_default(),
        permission_mode: ctx.permission_mode.clone(),
        agent_id: ctx.agent_id.clone(),
        agent_type: ctx.agent_type.clone(),
    }
}

#[cfg(test)]
#[path = "inputs.test.rs"]
mod tests;
