use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use crate::LlmMessage;
use crate::PermissionMode;
use crate::TokenUsage;
use crate::ToolId;

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
    /// Hidden from UI, visible to model.
    #[serde(default)]
    pub is_meta: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMessage {
    pub uuid: Uuid,
    pub message: LlmMessage,
    #[serde(default)]
    pub is_meta: bool,
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
