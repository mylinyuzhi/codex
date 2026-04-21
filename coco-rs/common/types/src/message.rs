use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::AttachmentBody;
use crate::AttachmentKind;
use crate::LlmMessage;
use crate::PermissionMode;
use crate::SilentPayload;
use crate::TokenUsage;
use crate::ToolId;
use crate::attachment_body::AlreadyReadFilePayload;
use crate::attachment_body::CommandPermissionsPayload;
use crate::attachment_body::DynamicSkillPayload;
use crate::attachment_body::EditedImageFilePayload;
use crate::attachment_body::HookCancelledPayload;
use crate::attachment_body::HookErrorDuringExecutionPayload;
use crate::attachment_body::HookNonBlockingErrorPayload;
use crate::attachment_body::HookPermissionDecisionPayload;
use crate::attachment_body::HookSystemMessagePayload;
use crate::attachment_body::StructuredOutputPayload;

/// Top-level message enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
    System(SystemMessage),
    Attachment(AttachmentMessage),
    ToolResult(ToolResultMessage),
    Progress(ProgressMessage),
    Tombstone(TombstoneMessage),
    ToolUseSummary(ToolUseSummaryMessage),
}

/// Visibility axes for a [`Message`] — orthogonal API × UI flags.
///
/// Mirrors the two-axis filter pattern: `normalizeAttachmentForAPI`
/// controls `api`, `nullRenderingAttachments.ts` controls `ui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visibility {
    /// Does this message's content reach the LLM API?
    pub api: bool,
    /// Does this message render in the user-visible UI transcript?
    pub ui: bool,
}

impl Visibility {
    pub const BOTH: Self = Self {
        api: true,
        ui: true,
    };
    pub const API_ONLY: Self = Self {
        api: true,
        ui: false,
    };
    pub const UI_ONLY: Self = Self {
        api: false,
        ui: true,
    };
    pub const NEITHER: Self = Self {
        api: false,
        ui: false,
    };
}

impl Message {
    pub fn uuid(&self) -> Option<&Uuid> {
        match self {
            Self::User(m) => Some(&m.uuid),
            Self::Assistant(m) => Some(&m.uuid),
            Self::System(m) => Some(m.uuid()),
            Self::Attachment(m) => Some(&m.uuid),
            Self::ToolResult(m) => Some(&m.uuid),
            Self::Progress(_) => None,
            Self::Tombstone(m) => Some(&m.uuid),
            Self::ToolUseSummary(m) => Some(&m.uuid),
        }
    }

    /// Single source of truth for API × UI filtering.
    ///
    /// - `User`/`Assistant`/`ToolResult` — both axes true unless the user
    ///   message is meta (hidden from UI only).
    /// - `System` — API-only (rendered as meta user on API, hidden from UI).
    /// - `Attachment` — derived from [`AttachmentKind`] predicates
    ///   ([`is_api_visible`](AttachmentKind::is_api_visible) +
    ///   [`renders_in_transcript`](AttachmentKind::renders_in_transcript)).
    /// - `Progress`/`ToolUseSummary` — UI-only.
    /// - `Tombstone` — neither (filtered in normalization).
    pub fn visibility(&self) -> Visibility {
        match self {
            // Human-typed user input is always both API-visible and
            // UI-visible. Reminder-injected "meta" user content lives in
            // `Message::Attachment` with an appropriate `AttachmentKind`.
            Self::User(_) | Self::Assistant(_) | Self::ToolResult(_) => Visibility::BOTH,
            Self::System(_) => Visibility::API_ONLY,
            Self::Attachment(a) => Visibility {
                api: a.kind.is_api_visible(),
                ui: a.kind.renders_in_transcript(),
            },
            Self::Progress(_) | Self::ToolUseSummary(_) => Visibility::UI_ONLY,
            Self::Tombstone(_) => Visibility::NEITHER,
        }
    }

    pub fn kind(&self) -> MessageKind {
        match self {
            Self::User(_) => MessageKind::User,
            Self::Assistant(_) => MessageKind::Assistant,
            Self::System(_) => MessageKind::System,
            Self::Attachment(_) => MessageKind::Attachment,
            Self::ToolResult(_) => MessageKind::ToolResult,
            Self::Progress(_) => MessageKind::Progress,
            Self::Tombstone(_) => MessageKind::Tombstone,
            Self::ToolUseSummary(_) => MessageKind::ToolUseSummary,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    /// LLM API layer — sent to API directly via .message field.
    pub message: LlmMessage,
    pub uuid: Uuid,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub is_visible_in_transcript_only: bool,
    /// Not sent to API.
    #[serde(default)]
    pub is_virtual: bool,
    #[serde(default)]
    pub is_compact_summary: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<MessageOrigin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantMessage {
    pub message: LlmMessage,
    pub uuid: Uuid,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<TokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_error: Option<ApiError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
}

/// API error attached to an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<i32>,
}

/// Attachment message: `kind` carries the TS-parity discriminant (60 variants),
/// `body` carries the typed payload.
///
/// **Invariant**: `kind` and `body` must agree — e.g. `kind = HookCancelled`
/// must come with `body = Silent(SilentPayload::HookCancelled(..))`. Do **not**
/// construct via struct literal; use the typed constructor helpers below.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMessage {
    pub uuid: Uuid,
    pub kind: AttachmentKind,
    pub body: AttachmentBody,
}

impl AttachmentMessage {
    /// Build an API-bound attachment (body is a pre-rendered `LlmMessage`).
    ///
    /// Covers reminder-produced text (in-crate) and outside-reminder text
    /// re-injections (compact / plan file reference). Caller picks the
    /// `kind`; `debug_assert` rejects kinds whose coverage is explicitly
    /// not API-visible.
    pub fn api(kind: AttachmentKind, message: LlmMessage) -> Self {
        debug_assert!(
            kind.is_api_visible(),
            "AttachmentMessage::api called with non-API-visible kind {kind:?}",
        );
        Self {
            uuid: Uuid::new_v4(),
            kind,
            body: AttachmentBody::Api(message),
        }
    }

    /// Build a unit attachment for feature-gated / runtime-bookkeeping kinds
    /// (no payload data).
    pub fn unit(kind: AttachmentKind) -> Self {
        debug_assert!(
            matches!(
                kind.coverage(),
                crate::Coverage::FeatureGated { .. } | crate::Coverage::RuntimeBookkeeping { .. }
            ),
            "AttachmentMessage::unit called with non-unit kind {kind:?}"
        );
        Self {
            uuid: Uuid::new_v4(),
            kind,
            body: AttachmentBody::Unit,
        }
    }

    // ── Silent constructors (one per SilentPayload variant) ──
    // Pattern: kind is fixed per helper, payload type-enforced.

    pub fn silent_hook_cancelled(payload: HookCancelledPayload) -> Self {
        Self::silent(
            AttachmentKind::HookCancelled,
            SilentPayload::HookCancelled(payload),
        )
    }
    pub fn silent_hook_error_during_execution(payload: HookErrorDuringExecutionPayload) -> Self {
        Self::silent(
            AttachmentKind::HookErrorDuringExecution,
            SilentPayload::HookErrorDuringExecution(payload),
        )
    }
    pub fn silent_hook_non_blocking_error(payload: HookNonBlockingErrorPayload) -> Self {
        Self::silent(
            AttachmentKind::HookNonBlockingError,
            SilentPayload::HookNonBlockingError(payload),
        )
    }
    pub fn silent_hook_system_message(payload: HookSystemMessagePayload) -> Self {
        Self::silent(
            AttachmentKind::HookSystemMessage,
            SilentPayload::HookSystemMessage(payload),
        )
    }
    pub fn silent_hook_permission_decision(payload: HookPermissionDecisionPayload) -> Self {
        Self::silent(
            AttachmentKind::HookPermissionDecision,
            SilentPayload::HookPermissionDecision(payload),
        )
    }
    pub fn silent_command_permissions(payload: CommandPermissionsPayload) -> Self {
        Self::silent(
            AttachmentKind::CommandPermissions,
            SilentPayload::CommandPermissions(payload),
        )
    }
    pub fn silent_structured_output(payload: StructuredOutputPayload) -> Self {
        Self::silent(
            AttachmentKind::StructuredOutput,
            SilentPayload::StructuredOutput(payload),
        )
    }
    pub fn silent_dynamic_skill(payload: DynamicSkillPayload) -> Self {
        Self::silent(
            AttachmentKind::DynamicSkill,
            SilentPayload::DynamicSkill(payload),
        )
    }
    pub fn silent_already_read_file(payload: AlreadyReadFilePayload) -> Self {
        Self::silent(
            AttachmentKind::AlreadyReadFile,
            SilentPayload::AlreadyReadFile(payload),
        )
    }
    pub fn silent_edited_image_file(payload: EditedImageFilePayload) -> Self {
        Self::silent(
            AttachmentKind::EditedImageFile,
            SilentPayload::EditedImageFile(payload),
        )
    }

    /// Internal — callers use the typed silent_* helpers above.
    fn silent(kind: AttachmentKind, payload: SilentPayload) -> Self {
        Self {
            uuid: Uuid::new_v4(),
            kind,
            body: AttachmentBody::Silent(payload),
        }
    }

    /// API-bound `LlmMessage` if this attachment carries one.
    /// Returns `Some` only for [`AttachmentBody::Api`] bodies;
    /// `Silent` / `Unit` bodies never reach the API.
    pub fn as_api_message(&self) -> Option<&LlmMessage> {
        match &self.body {
            AttachmentBody::Api(m) => Some(m),
            AttachmentBody::Silent(_) | AttachmentBody::Unit => None,
        }
    }

    /// UI-facing text rendering of this attachment.
    ///
    /// Used by transcript / log extraction helpers. Returns empty string
    /// for bodies that have no textual representation (structured silent
    /// payloads rely on dedicated renderers).
    pub fn as_text_for_display(&self) -> String {
        match &self.body {
            AttachmentBody::Api(m) => llm_message_text(m),
            AttachmentBody::Silent(_) | AttachmentBody::Unit => String::new(),
        }
    }
}

/// Extract text content from an `LlmMessage` (simple concatenation of any
/// `TextContent` parts). Lives here so `AttachmentMessage::as_text_for_display`
/// can reuse it without pulling in `coco_messages`.
fn llm_message_text(msg: &LlmMessage) -> String {
    use crate::AssistantContent;
    use crate::LlmMessage as L;
    use crate::UserContent;
    match msg {
        L::User { content, .. } => content
            .iter()
            .filter_map(|p| match p {
                UserContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        L::Assistant { content, .. } => content
            .iter()
            .filter_map(|p| match p {
                AssistantContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub uuid: Uuid,
    pub message: LlmMessage,
    pub tool_use_id: String,
    pub tool_id: ToolId,
    #[serde(default)]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressMessage {
    pub tool_use_id: String,
    pub data: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_message_uuid: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TombstoneMessage {
    pub uuid: Uuid,
    pub original_kind: MessageKind,
}

/// Which message variant was tombstoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    User,
    Assistant,
    System,
    Attachment,
    ToolResult,
    Progress,
    Tombstone,
    ToolUseSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUseSummaryMessage {
    pub uuid: Uuid,
    pub tool_id: ToolId,
    pub summary: String,
}

/// System messages have sub-types for different notification kinds.
/// All system messages are `role: "user"` with `is_meta: true` for the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SystemMessage {
    Informational(SystemInformationalMessage),
    ApiError(SystemApiErrorMessage),
    CompactBoundary(SystemCompactBoundaryMessage),
    MicrocompactBoundary(SystemMicrocompactBoundaryMessage),
    LocalCommand(SystemLocalCommandMessage),
    PermissionRetry(SystemPermissionRetryMessage),
    BridgeStatus(SystemBridgeStatusMessage),
    MemorySaved(SystemMemorySavedMessage),
    AwaySummary(SystemAwaySummaryMessage),
    AgentsKilled(SystemAgentsKilledMessage),
    ApiMetrics(SystemApiMetricsMessage),
    StopHookSummary(SystemStopHookSummaryMessage),
    TurnDuration(SystemTurnDurationMessage),
    ScheduledTaskFire(SystemScheduledTaskFireMessage),
}

impl SystemMessage {
    pub fn uuid(&self) -> &Uuid {
        match self {
            Self::Informational(m) => &m.uuid,
            Self::ApiError(m) => &m.uuid,
            Self::CompactBoundary(m) => &m.uuid,
            Self::MicrocompactBoundary(m) => &m.uuid,
            Self::LocalCommand(m) => &m.uuid,
            Self::PermissionRetry(m) => &m.uuid,
            Self::BridgeStatus(m) => &m.uuid,
            Self::MemorySaved(m) => &m.uuid,
            Self::AwaySummary(m) => &m.uuid,
            Self::AgentsKilled(m) => &m.uuid,
            Self::ApiMetrics(m) => &m.uuid,
            Self::StopHookSummary(m) => &m.uuid,
            Self::TurnDuration(m) => &m.uuid,
            Self::ScheduledTaskFire(m) => &m.uuid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemMessageLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInformationalMessage {
    pub uuid: Uuid,
    pub level: SystemMessageLevel,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemApiErrorMessage {
    pub uuid: Uuid,
    pub error: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<i32>,
}

/// How compaction was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactTrigger {
    Manual,
    Auto,
}

/// Preserved message segment after partial compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreservedSegment {
    /// First kept message UUID.
    pub head_uuid: Uuid,
    /// Summary or boundary anchor UUID.
    pub anchor_uuid: Uuid,
    /// Last kept message UUID.
    pub tail_uuid: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemCompactBoundaryMessage {
    pub uuid: Uuid,
    pub tokens_before: i64,
    pub tokens_after: i64,
    /// How compaction was triggered.
    #[serde(default = "default_compact_trigger")]
    pub trigger: CompactTrigger,
    /// User-supplied context for the compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_context: Option<String>,
    /// Number of messages that were summarized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub messages_summarized: Option<i32>,
    /// Tools discovered before compaction (for delta re-announcement).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_compact_discovered_tools: Vec<String>,
    /// Preserved segment for partial/suffix-preserving compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_segment: Option<PreservedSegment>,
}

fn default_compact_trigger() -> CompactTrigger {
    CompactTrigger::Auto
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMicrocompactBoundaryMessage {
    pub uuid: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemLocalCommandMessage {
    pub uuid: Uuid,
    pub command: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPermissionRetryMessage {
    pub uuid: Uuid,
    pub tool_name: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemBridgeStatusMessage {
    pub uuid: Uuid,
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMemorySavedMessage {
    pub uuid: Uuid,
    pub memory_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemAwaySummaryMessage {
    pub uuid: Uuid,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemAgentsKilledMessage {
    pub uuid: Uuid,
    pub count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemApiMetricsMessage {
    pub uuid: Uuid,
    pub usage: TokenUsage,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemStopHookSummaryMessage {
    pub uuid: Uuid,
    pub hook_name: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemTurnDurationMessage {
    pub uuid: Uuid,
    pub duration_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemScheduledTaskFireMessage {
    pub uuid: Uuid,
    pub task_id: String,
    pub schedule: String,
}

/// Where a message originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    UserInput,
    SystemInjected,
    ToolResult,
    CompactSummary,
    SubagentReply,
}

/// Direction hint for partial compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartialCompactDirection {
    Oldest,
    Newest,
}

#[cfg(test)]
#[path = "message.test.rs"]
mod tests;
