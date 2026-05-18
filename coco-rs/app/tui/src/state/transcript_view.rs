//! Derived view of engine `MessageHistory` for the TUI.
//!
//! Authority remains with `coco_messages::MessageHistory` in the
//! engine; this struct is a TUI-side **pure derivation** rebuilt
//! incrementally from `ServerNotification::MessageAppended` /
//! `MessageTruncated` / `SessionResetForResume` events. See
//! `engine-tui-unified-transcript-plan.md` §6.1.
//!
//! Phase 3a status: this view is populated alongside the legacy
//! `session.messages: Vec<ChatMessage>` but renderers still read from
//! the legacy field. Phase 3b will flip render path to read here and
//! delete the legacy field. The dual-write is intentional and
//! temporary; do not rely on it in long-running designs.
//!
//! Per-cell render layout (`cached_lines`, `cached_height`) is
//! intentionally not part of this struct yet. Layout caching lives in
//! the renderer once it switches over (Phase 3b).

use std::collections::HashMap;
use std::sync::Arc;

use coco_messages::Message;
use coco_messages::SystemMessage;
use uuid::Uuid;

use super::derive::message_to_cells;

/// Append-only-with-truncation list of derived cells.
#[derive(Debug, Default)]
pub struct TranscriptView {
    cells: Vec<RenderedCell>,
    /// First cell index per source message UUID. One `Message` may
    /// derive multiple `RenderedCell`s (e.g. `Assistant` with text +
    /// thinking + tool_use blocks); the index points at the head cell
    /// of that group.
    by_uuid: HashMap<Uuid, usize>,
}

impl TranscriptView {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cells(&self) -> &[RenderedCell] {
        &self.cells
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn find_head_index_by_uuid(&self, uuid: &Uuid) -> Option<usize> {
        self.by_uuid.get(uuid).copied()
    }

    /// Append cells derived from `msg`. Multiple cells may be produced
    /// for one Message (e.g. assistant text + tool_use blocks); the
    /// UUID index records the first cell so consumers can find the
    /// group head. The owned `Arc<Message>` is shared into each cell
    /// so renderers can recover engine-side fields (`is_meta`,
    /// `permission_mode`, timestamp, …) without re-serializing.
    pub fn on_message_appended(&mut self, msg: Arc<Message>) {
        let derived = message_to_cells(msg.clone());
        if derived.is_empty() {
            return;
        }
        let head_idx = self.cells.len();
        if let Some(uuid) = msg.uuid() {
            self.by_uuid.insert(*uuid, head_idx);
        }
        self.cells.extend(derived);
    }

    /// Truncate to the first `keep_count` ENGINE messages. Because one
    /// engine `Message` may have produced multiple cells, this walks
    /// `by_uuid` to find the cell index where engine-message
    /// `keep_count` begins and drops the tail.
    ///
    /// Phase 3a simplification: when the truncation target UUID can't
    /// be resolved (e.g. resume hasn't populated by_uuid yet), clamp
    /// by `keep_count` directly. Resume + auto-restore both go through
    /// the same path so this is robust enough.
    pub fn on_message_truncated(&mut self, keep_count: usize) {
        // Walk by_uuid to find the smallest cell index whose source
        // message had position >= keep_count. Since by_uuid maps the
        // engine message to its head cell index but doesn't carry the
        // engine message index, we approximate: count distinct head
        // UUIDs and stop when we've kept `keep_count` of them.
        let mut seen_heads = 0usize;
        let mut cut: Option<usize> = None;
        let mut last_uuid: Option<Uuid> = None;
        for (i, cell) in self.cells.iter().enumerate() {
            if last_uuid != Some(cell.message_uuid) {
                last_uuid = Some(cell.message_uuid);
                if seen_heads == keep_count {
                    cut = Some(i);
                    break;
                }
                seen_heads += 1;
            }
        }
        if let Some(c) = cut {
            self.cells.truncate(c);
            self.rebuild_index();
        }
    }

    pub fn on_session_reset(&mut self) {
        self.cells.clear();
        self.by_uuid.clear();
    }

    fn rebuild_index(&mut self) {
        self.by_uuid.clear();
        let mut last_uuid: Option<Uuid> = None;
        for (i, cell) in self.cells.iter().enumerate() {
            if last_uuid != Some(cell.message_uuid) {
                self.by_uuid.insert(cell.message_uuid, i);
                last_uuid = Some(cell.message_uuid);
            }
        }
    }
}

/// One render cell derived from a (possibly partial) engine `Message`.
/// Carries an `Arc<Message>` back-pointer so renderers can extract
/// engine-authoritative fields (`is_meta`, `permission_mode`,
/// `timestamp`, `is_compact_summary`, …) without parallel storage.
/// Layout / viewport-dependent fields are intentionally absent —
/// layout caching lives in the renderer at draw time.
#[derive(Debug, Clone)]
pub struct RenderedCell {
    pub message_uuid: Uuid,
    pub kind: CellKind,
    pub source: Arc<Message>,
}

/// TUI-internal classification used for render dispatch.
///
/// Mirrors but is not identical to `coco_messages::Message` variants —
/// several `SystemMessage` sub-variants may render the same way, and
/// `AssistantMessage` content blocks map to multiple `CellKind`s
/// (text + thinking + tool_use).
///
/// Phase 3a keeps `CellKind` flat with enough fidelity to drive a
/// future renderer; field-level rendering data (markdown AST cache,
/// diff hunks, etc.) is not stored here per layer-hygiene rule from
/// `engine-tui-unified-transcript-plan.md` §2.
#[derive(Debug, Clone)]
pub enum CellKind {
    /// User text input.
    UserText { text: String },
    /// User attachment image / paste.
    UserAttachment,
    /// Assistant text fragment.
    AssistantText { text: String, model: String },
    /// Assistant reasoning / thinking content.
    AssistantThinking { text: String },
    /// Assistant redacted thinking (encrypted, displayed as opaque).
    AssistantRedactedThinking,
    /// Assistant `tool_use` content block.
    ToolUse { call_id: String, tool_name: String },
    /// Tool result returned to the model.
    ToolResult { call_id: String },
    /// Attachment message (system-reminder-wrapped queued command,
    /// hook payload, etc.).
    Attachment,
    /// Tool-use summary (Fast-role-generated mobile label).
    ToolUseSummary { summary: String },
    /// Progress meta-message (transient, often filtered).
    Progress,
    /// Tombstoned message (filtered from rendering normally).
    Tombstone,
    /// System message — fine-grained kind drives render style.
    System(SystemCellKind),
}

/// Render-side classification of `SystemMessage` sub-variants.
///
/// Phase 3a includes only the kinds the engine actively emits. New
/// variants land alongside their `SystemMessage` extension.
#[derive(Debug, Clone)]
pub enum SystemCellKind {
    /// User cancellation — renders dim "Interrupted · …" row.
    /// `for_tool_use` is read from the engine-authoritative field and
    /// never recomputed (eliminates the prior engine ↔ TUI race).
    UserInterruption { for_tool_use: bool },
    /// Generic informational system row (level + title + body).
    Informational,
    /// API error reported by the engine.
    ApiError,
    /// Compaction boundary marker.
    CompactBoundary,
    /// Micro-compaction boundary (rare; usually filtered).
    MicrocompactBoundary,
    /// Local /command output preserved in transcript.
    LocalCommand,
    /// Permission-retry banner.
    PermissionRetry,
    /// IDE bridge connection status.
    BridgeStatus,
    /// Memory file saved (extract / dream).
    MemorySaved,
    /// Away-summary system row.
    AwaySummary,
    /// Agents killed system row.
    AgentsKilled,
    /// API metrics tail row.
    ApiMetrics,
    /// Stop-hook summary row.
    StopHookSummary,
    /// Turn duration row.
    TurnDuration,
    /// Scheduled task fire row.
    ScheduledTaskFire,
}

impl From<&SystemMessage> for SystemCellKind {
    fn from(m: &SystemMessage) -> Self {
        match m {
            SystemMessage::UserInterruption(i) => Self::UserInterruption {
                for_tool_use: i.for_tool_use,
            },
            SystemMessage::Informational(_) => Self::Informational,
            SystemMessage::ApiError(_) => Self::ApiError,
            SystemMessage::CompactBoundary(_) => Self::CompactBoundary,
            SystemMessage::MicrocompactBoundary(_) => Self::MicrocompactBoundary,
            SystemMessage::LocalCommand(_) => Self::LocalCommand,
            SystemMessage::PermissionRetry(_) => Self::PermissionRetry,
            SystemMessage::BridgeStatus(_) => Self::BridgeStatus,
            SystemMessage::MemorySaved(_) => Self::MemorySaved,
            SystemMessage::AwaySummary(_) => Self::AwaySummary,
            SystemMessage::AgentsKilled(_) => Self::AgentsKilled,
            SystemMessage::ApiMetrics(_) => Self::ApiMetrics,
            SystemMessage::StopHookSummary(_) => Self::StopHookSummary,
            SystemMessage::TurnDuration(_) => Self::TurnDuration,
            SystemMessage::ScheduledTaskFire(_) => Self::ScheduledTaskFire,
        }
    }
}
