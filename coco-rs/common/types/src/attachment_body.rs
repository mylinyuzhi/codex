//! Typed payload carrier for [`AttachmentMessage`](crate::AttachmentMessage).
//!
//! Pairs with [`AttachmentKind`](crate::AttachmentKind) (the 60-variant TS-parity
//! discriminant): `kind` classifies per TS `Attachment.type`, `body` carries the
//! data. `AttachmentBody` variants cover only kinds coco-rs actually produces,
//! so `FeatureGated` / `RuntimeBookkeeping` kinds don't pollute the payload surface.
//!
//! # Invariant
//!
//! `kind` and `body` must agree — e.g. `AttachmentKind::HookCancelled` requires
//! `AttachmentBody::Silent(SilentPayload::HookCancelled(..))`. The constructor
//! helpers on [`AttachmentMessage`](crate::AttachmentMessage) enforce this; do
//! not construct by struct literal.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::HookEventType;
use crate::LlmMessage;

// ─── AttachmentBody ─────────────────────────────────────────────────────

/// Typed payload for an [`AttachmentMessage`](crate::AttachmentMessage).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "body", rename_all = "snake_case")]
pub enum AttachmentBody {
    /// Pre-rendered `LlmMessage` — reaches the model when filtered in.
    Api(LlmMessage),
    /// Typed silent payload — UI/transcript only, never sent to the API.
    Silent(SilentPayload),
    /// Discriminator-only — for `FeatureGated` / `RuntimeBookkeeping` kinds.
    Unit,
}

// ─── Silent payloads (one per silent AttachmentKind) ────────────────────

/// Typed payload for silent attachment kinds.
///
/// Variant names map 1:1 to the [`AttachmentKind`](crate::AttachmentKind)
/// silent variants. Adding a new silent kind requires adding a matching
/// variant here — enforced by the constructor helpers on
/// [`AttachmentMessage`](crate::AttachmentMessage) +
/// the `silent_kind_round_trips_through_payload` parity test.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SilentPayload {
    // ── Silent events (Coverage::SilentEvent — 8 variants) ──
    HookCancelled(HookCancelledPayload),
    HookErrorDuringExecution(HookErrorDuringExecutionPayload),
    HookNonBlockingError(HookNonBlockingErrorPayload),
    HookSystemMessage(HookSystemMessagePayload),
    HookPermissionDecision(HookPermissionDecisionPayload),
    CommandPermissions(CommandPermissionsPayload),
    StructuredOutput(StructuredOutputPayload),
    DynamicSkill(DynamicSkillPayload),

    // ── Silent reminders (Coverage::SilentReminder — in-crate) ──
    AlreadyReadFile(AlreadyReadFilePayload),
    EditedImageFile(EditedImageFilePayload),
}

/// TS parity: `HookCancelledAttachment` (`utils/attachments.ts:396-403`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookCancelledPayload {
    pub hook_name: String,
    pub tool_use_id: String,
    pub hook_event: HookEventType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
}

/// TS parity: `HookErrorDuringExecutionAttachment` (`utils/attachments.ts:405-414`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookErrorDuringExecutionPayload {
    pub content: String,
    pub hook_name: String,
    pub tool_use_id: String,
    pub hook_event: HookEventType,
}

/// TS parity: `HookNonBlockingErrorAttachment` (`utils/attachments.ts:429+`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookNonBlockingErrorPayload {
    pub error: String,
    pub hook_name: String,
    pub tool_use_id: String,
    pub hook_event: HookEventType,
}

/// TS parity: `HookSystemMessageAttachment` (`utils/attachments.ts:388-394`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookSystemMessagePayload {
    pub content: String,
    pub hook_name: String,
    pub tool_use_id: String,
    pub hook_event: HookEventType,
}

/// TS parity: `HookPermissionDecisionAttachment` (`utils/attachments.ts:381-386`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookPermissionDecisionPayload {
    pub decision: HookPermissionDecision,
    pub tool_use_id: String,
    pub hook_event: HookEventType,
}

/// `allow` / `deny` decision, matching TS `'allow' | 'deny'`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HookPermissionDecision {
    #[default]
    Allow,
    Deny,
}

/// TS parity: `command_permissions` (`utils/attachments.ts:605-608`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CommandPermissionsPayload {
    pub allowed_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// TS parity: `structured_output` (`utils/attachments.ts:639+`,
/// `services/tools/toolExecution.ts:1276`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StructuredOutputPayload {
    pub tool_name: String,
    pub tool_use_id: String,
    pub data: serde_json::Value,
}

/// TS parity: `dynamic_skill` (`utils/attachments.ts:525+`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DynamicSkillPayload {
    pub skill_name: String,
    pub path: String,
}

/// TS parity: `AlreadyReadFileAttachment` (`utils/attachments.ts:323-333`).
///
/// TS carries the (potentially truncated) file content inline for UI display
/// even though `normalizeAttachmentForAPI` returns `[]`. coco-rs follows
/// suit — `content` is the last-known file body used by transcript viewers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct AlreadyReadFilePayload {
    /// Absolute or resolved path (engine-populated).
    pub filename: PathBuf,
    /// Path relative to CWD at creation time, for stable display.
    pub display_path: String,
    /// Cached content from `FileReadState` at dedup time.
    #[serde(default)]
    pub content: String,
    /// Whether the content was truncated due to size limits.
    #[serde(default)]
    pub truncated: bool,
}

/// TS parity: `edited_image_file` (`utils/attachments.ts:456-460`).
///
/// Image bytes can't be diffed textually; the UI renders a marker / thumbnail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EditedImageFilePayload {
    pub filename: PathBuf,
    /// Path relative to CWD at creation time.
    pub display_path: String,
}

#[cfg(test)]
#[path = "attachment_body.test.rs"]
mod tests;
