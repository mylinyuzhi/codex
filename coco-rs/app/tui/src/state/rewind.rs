//! Rewind overlay state — MessageSelector equivalent from TS.
//!
//! TS: src/components/MessageSelector.tsx
//!
//! The overlay has two phases: MessageSelect (pick a user message)
//! and RestoreOptions (choose what to restore). Confirming is a
//! transient phase shown while the rewind executes.

use coco_types::PermissionMode;

/// Phase of the rewind overlay flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewindPhase {
    /// Picking which user message to rewind to.
    MessageSelect,
    /// Choosing what to restore after picking a message.
    RestoreOptions,
    /// Executing the rewind (loading indicator).
    Confirming,
}

/// What to restore during rewind.
///
/// TS: RestoreOption = 'both' | 'conversation' | 'code' | 'summarize' | ...
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreType {
    /// Restore both code (file history) and conversation (truncate messages).
    Both,
    /// Restore conversation only (truncate messages, leave files as-is).
    ConversationOnly,
    /// Restore code only (file history rewind, keep conversation).
    CodeOnly,
}

impl RestoreType {
    /// Localized label resolved against the active locale at render time.
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Both => crate::i18n::t!("rewind.option_code_and_conv"),
            Self::ConversationOnly => crate::i18n::t!("rewind.option_conv_only"),
            Self::CodeOnly => crate::i18n::t!("rewind.option_code_only"),
        }
    }
}

/// A user message that can be rewound to.
///
/// TS: MessageSelector shows user messages with truncated content,
/// relative timestamps, and file change counts.
#[derive(Debug, Clone)]
pub struct RewindableMessage {
    /// UUID of the user message.
    pub message_id: String,
    /// Index in the full messages vec (for display ordering).
    pub message_index: i32,
    /// Truncated display text (first 50 chars of user input).
    pub display_text: String,
    /// Turn label (e.g. "Turn 3").
    pub turn_label: String,
    /// Permission mode active when this message was created.
    pub permission_mode: Option<PermissionMode>,
}

/// Rewind overlay state.
///
/// TS: MessageSelector component state (selectedIndex, messageToRestore,
/// selectedRestoreOption, diffStatsForRestore, etc.)
#[derive(Debug, Clone)]
pub struct RewindOverlay {
    /// Current flow phase.
    pub phase: RewindPhase,
    /// User messages available for rewind (newest last).
    pub messages: Vec<RewindableMessage>,
    /// Selected message index in the messages list.
    pub selected: i32,
    /// Selected restore option index (RestoreOptions phase).
    pub option_selected: i32,
    /// Available restore options for the selected message.
    pub available_options: Vec<RestoreType>,
    /// Diff stats preview for the selected message.
    pub diff_stats: Option<DiffStatsPreview>,
    /// Whether file history is enabled for this session.
    pub file_history_enabled: bool,
    /// Whether file history has changes for selected message.
    pub has_file_changes: bool,
}

/// Preview of what file rewind would change.
///
/// TS: DiffStats from fileHistory.ts
#[derive(Debug, Clone, Default)]
pub struct DiffStatsPreview {
    pub files_changed: i32,
    pub insertions: i64,
    pub deletions: i64,
}

/// Build available restore options based on file history state.
///
/// TS: getRestoreOptions(canRestoreCode) in MessageSelector.tsx
pub fn build_restore_options(
    file_history_enabled: bool,
    has_file_changes: bool,
) -> Vec<RestoreType> {
    if file_history_enabled && has_file_changes {
        vec![
            RestoreType::Both,
            RestoreType::ConversationOnly,
            RestoreType::CodeOnly,
        ]
    } else {
        vec![RestoreType::ConversationOnly]
    }
}

#[cfg(test)]
#[path = "rewind.test.rs"]
mod tests;
