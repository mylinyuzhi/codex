//! Extended types ported from TypeScript source.
//!
//! Contains types that complement the core types in sibling modules.
//! Organized by origin file: hooks, command, permission, log/transcript.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use uuid::Uuid;

use crate::HookEventType;
use crate::Message;
use crate::PermissionDecision;
use crate::PermissionMode;
use crate::PermissionRule;
use crate::PermissionUpdate;

// ============================================================================
// Hook Extended Types (from hooks.ts)
// ============================================================================

/// Progress report from a running hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookProgress {
    pub hook_event: HookEventType,
    pub hook_name: String,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

/// Error from a blocking hook that prevents continuation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookBlockingError {
    pub blocking_error: String,
    pub command: String,
}

/// Result of a permission request hook (allow or deny).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum PermissionRequestResult {
    Allow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_permissions: Option<Vec<PermissionUpdate>>,
    },
    Deny {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default)]
        interrupt: bool,
    },
}

// NOTE: HookResultExt, HookOutcomeExt, and the aspirational AggregatedHookResult
// that were previously here have been removed. The canonical types are:
//   - HookOutcome in coco_types::hook (4 variants: Success/Blocking/NonBlockingError/Cancelled)
//   - AggregatedHookResult in coco_hooks::orchestration (the implementation)

/// Prompt elicitation request from a hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequest {
    /// Request ID.
    pub prompt: String,
    pub message: String,
    pub options: Vec<PromptOption>,
}

/// A single option in a prompt elicitation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptOption {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Response to a prompt elicitation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponse {
    /// Request ID (mirrors PromptRequest.prompt).
    pub prompt_response: String,
    pub selected: String,
}

// ============================================================================
// Command Extended Types (from command.ts)
// ============================================================================

/// How a command result should be displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandResultDisplay {
    Skip,
    System,
    User,
}

/// Entrypoint for session resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResumeEntrypoint {
    CliFlag,
    SlashCommandPicker,
    SlashCommandSessionId,
    SlashCommandTitle,
    Fork,
}

/// Distinguishes workflow-backed commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    Workflow,
}

/// Extended command base with fields from TS CommandBase not in the core CommandBase.
///
/// Meant to be composed alongside `CommandBase` when the full TS shape is needed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandBaseExt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default)]
    pub disable_model_invocation: bool,
    #[serde(default)]
    pub has_user_specified_description: bool,
    #[serde(default)]
    pub is_mcp: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<CommandKind>,
    #[serde(default)]
    pub immediate: bool,
    /// Display name override (when different from `name`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_facing_name: Option<String>,
}

/// Extended prompt command data with fields from TS PromptCommand.
///
/// Supplements `PromptCommandData` when the full TS shape is needed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptCommandDataExt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arg_names: Vec<String>,
    #[serde(default)]
    pub disable_non_interactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_root: Option<String>,
    /// Glob patterns for file paths this skill applies to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

// ============================================================================
// Permission Extended Types (from permissions.ts)
// ============================================================================

/// Extended permission decision reason variants from TS.
///
/// Supplements the core `PermissionDecisionReason` with variants that
/// are used in the full permission evaluation pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PermissionDecisionReasonExt {
    Rule {
        rule: PermissionRule,
    },
    Mode {
        mode: PermissionMode,
    },
    Classifier {
        classifier: String,
        reason: String,
    },
    Hook {
        hook_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hook_source: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    SafetyCheck {
        reason: String,
        classifier_approvable: bool,
    },
    AsyncAgent {
        reason: String,
    },
    SubcommandResults {
        /// Keyed by subcommand name.
        reasons: HashMap<String, PermissionDecision>,
    },
    PermissionPromptTool {
        permission_prompt_tool_name: String,
        tool_result: serde_json::Value,
    },
    SandboxOverride {
        reason: SandboxOverrideReason,
    },
    WorkingDir {
        reason: String,
    },
    Other {
        reason: String,
    },
}

/// Specific reasons for a sandbox override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxOverrideReason {
    ExcludedCommand,
    DangerouslyDisableSandbox,
}

/// Permission result with passthrough option (extends PermissionDecision).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "behavior", rename_all = "snake_case")]
pub enum PermissionResult {
    Allow {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
        #[serde(default)]
        user_modified: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision_reason: Option<PermissionDecisionReasonExt>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        accept_feedback: Option<String>,
    },
    Ask {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_input: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision_reason: Option<PermissionDecisionReasonExt>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        suggestions: Vec<PermissionUpdate>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocked_path: Option<String>,
    },
    Deny {
        message: String,
        decision_reason: PermissionDecisionReasonExt,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_use_id: Option<String>,
    },
    Passthrough {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        decision_reason: Option<PermissionDecisionReasonExt>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        suggestions: Vec<PermissionUpdate>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocked_path: Option<String>,
    },
}

/// Extended tool permission context fields from TS.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolPermissionContextExt {
    #[serde(default)]
    pub should_avoid_permission_prompts: bool,
    #[serde(default)]
    pub await_automated_checks_before_dialog: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_plan_mode: Option<PermissionMode>,
}

/// Minimal command shape for permission metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionCommandMetadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Risk level for permission explanations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    #[serde(rename = "LOW")]
    Low,
    #[serde(rename = "MEDIUM")]
    Medium,
    #[serde(rename = "HIGH")]
    High,
}

/// Human-readable explanation of a permission decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionExplanation {
    pub risk_level: RiskLevel,
    pub explanation: String,
    pub reasoning: String,
    pub risk: String,
}

// ============================================================================
// Log / Transcript Extended Types (from logs.ts)
// ============================================================================

/// Summary message for session compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryEntry {
    pub leaf_uuid: Uuid,
    pub summary: String,
}

/// User-set custom title for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomTitleEntry {
    pub session_id: Uuid,
    pub custom_title: String,
}

/// AI-generated session title (distinct from user-set titles).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiTitleEntry {
    pub session_id: Uuid,
    pub ai_title: String,
}

/// Tag for session search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagEntry {
    pub session_id: Uuid,
    pub tag: String,
}

/// Agent name assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNameEntry {
    pub session_id: Uuid,
    pub agent_name: String,
}

/// Agent color assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentColorEntry {
    pub session_id: Uuid,
    pub agent_color: String,
}

/// Agent setting reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettingEntry {
    pub session_id: Uuid,
    pub agent_setting: String,
}

/// Periodic fork-generated summary of current agent activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummaryEntry {
    pub session_id: Uuid,
    pub summary: String,
    pub timestamp: String,
}

/// PR link stored in session transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrLinkEntry {
    pub session_id: Uuid,
    pub pr_number: i32,
    pub pr_url: String,
    /// "owner/repo" format.
    pub pr_repository: String,
    pub timestamp: String,
}

/// Session mode entry (coordinator or normal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    Coordinator,
    Normal,
}

/// Persisted worktree session state for resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedWorktreeSession {
    pub original_cwd: String,
    pub worktree_path: String,
    pub worktree_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_head_commit: Option<String>,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux_session_name: Option<String>,
    #[serde(default)]
    pub hook_based: bool,
}

/// Per-file attribution state tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAttributionState {
    pub content_hash: String,
    pub claude_contribution: i64,
    pub mtime: i64,
}

/// Attribution snapshot for commit attribution tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributionSnapshotEntry {
    pub message_id: Uuid,
    pub surface: String,
    pub file_states: HashMap<String, FileAttributionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_count_at_last_commit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_prompt_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_prompt_count_at_last_commit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escape_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escape_count_at_last_commit: Option<i64>,
}

/// Full transcript message (SerializedMessage + transcript metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMessage {
    pub message: Message,
    pub cwd: String,
    pub user_type: String,
    pub session_id: String,
    pub timestamp: String,
    pub version: String,
    pub parent_uuid: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_parent_uuid: Option<Uuid>,
    #[serde(default)]
    pub is_sidechain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
}

/// Discriminated union of all transcript entry types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEntry {
    Transcript(Box<TranscriptMessage>),
    Summary(SummaryEntry),
    CustomTitle(CustomTitleEntry),
    AiTitle(AiTitleEntry),
    Tag(TagEntry),
    AgentName(AgentNameEntry),
    AgentColor(AgentColorEntry),
    AgentSetting(AgentSettingEntry),
    TaskSummary(TaskSummaryEntry),
    PrLink(PrLinkEntry),
    AttributionSnapshot(AttributionSnapshotEntry),
}
