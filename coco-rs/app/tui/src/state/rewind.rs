//! Rewind state state — MessageSelector equivalent from TS.
//!
//! TS: src/components/MessageSelector.tsx
//!
//! The Rust state machine has four phases: MessageSelect (pick a user
//! message), RestoreOptions (choose what to restore), SummarizeFeedback
//! (Rust's dedicated input phase for TS's inline option input), and
//! Confirming (shown while rewind executes).

use coco_types::PermissionMode;
use coco_types::RewindDiffStatsPayload;

/// Phase of the rewind state flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewindPhase {
    /// Picking which user message to rewind to.
    MessageSelect,
    /// Choosing what to restore after picking a message.
    RestoreOptions,
    /// Optional free-text feedback box shown when the user picks a
    /// Summarize variant. TS: `MessageSelector.tsx:107-128` renders
    /// the input inside the option list itself; we use a dedicated
    /// phase to keep the state machine explicit.
    SummarizeFeedback,
    /// Executing the rewind (loading indicator).
    Confirming,
}

/// What to restore during rewind.
///
/// TS: RestoreOption = 'both' | 'conversation' | 'code' | 'summarize' |
/// 'summarize_up_to' | 'nevermind' (`MessageSelector.tsx:31`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreType {
    /// Restore both code (file history) and conversation (truncate messages).
    Both,
    /// Restore conversation only (truncate messages, leave files as-is).
    ConversationOnly,
    /// Restore code only (file history rewind, keep conversation).
    CodeOnly,
    /// Summarize messages from the picked message onward; keep the
    /// prefix as-is, replace the suffix with a single summary
    /// message. Direction = `from` in TS.
    SummarizeFrom { feedback: Option<String> },
    /// Summarize messages up to the picked message (exclusive); keep
    /// subsequent messages as-is. Direction = `up_to` in TS. Gated
    /// by `settings.rewind.allow_summarize_up_to` (default false) to
    /// match TS's Anthropic-only `'external' === 'ant'` gating.
    SummarizeUpTo { feedback: Option<String> },
    /// Cancel selection — go back to the message list. Mirrors TS's
    /// `'nevermind'` option (`MessageSelector.tsx:129-132`). Selecting
    /// it never reaches `handle_rewind`; the confirm handler routes it
    /// back to `RewindPhase::MessageSelect`.
    Nevermind,
}

impl RestoreType {
    /// Localized label resolved against the active locale at render time.
    pub fn label(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Both => crate::i18n::t!("rewind.option_code_and_conv"),
            Self::ConversationOnly => crate::i18n::t!("rewind.option_conv_only"),
            Self::CodeOnly => crate::i18n::t!("rewind.option_code_only"),
            Self::SummarizeFrom { .. } => crate::i18n::t!("rewind.option_summarize_from"),
            Self::SummarizeUpTo { .. } => crate::i18n::t!("rewind.option_summarize_up_to"),
            Self::Nevermind => crate::i18n::t!("rewind.option_nevermind"),
        }
    }

    /// Cheap discriminant matcher that ignores variant payload —
    /// used by the UI to compare focus state without cloning the
    /// feedback string.
    pub fn variant_eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// A user message that can be rewound to.
///
/// TS: MessageSelector shows user messages with truncated content,
/// relative timestamps, and file change counts.
#[derive(Debug, Clone)]
pub struct RewindableMessage {
    /// UUID of the user message. `None` for the synthetic `(current)`
    /// row appended by `build_rewind_state` — `is_synthetic()` derives
    /// the "is this the current-prompt anchor?" check, so there is no
    /// separate boolean flag to keep in sync. Stored as `Uuid` (not
    /// `String`) so preselect comparison is case-insensitive on input
    /// — `Uuid::parse_str` accepts both cases, `Uuid` equality is
    /// structural.
    ///
    /// TS uses a fresh random UUID + `messages.includes(picked)` to
    /// distinguish synthetic from real (`MessageSelector.tsx:60-66`,
    /// :165). Rust uses `Option` for the same effect with one fewer
    /// field.
    pub message_id: Option<uuid::Uuid>,
    /// Index in the full messages vec (for display ordering). `-1`
    /// for the synthetic `(current)` row.
    pub message_index: i32,
    /// Display text after TS-equivalent prompt/display tag handling.
    /// Width-dependent truncation belongs in presentation.
    pub display_text: String,
    /// Pre-rendered relative timestamp ("3 minutes ago"). Computed
    /// at state-build time so the picker render doesn't need a
    /// clock. TS: `formatRelativeTimeAgo(message.timestamp)`
    /// (`MessageSelector.tsx:336`).
    pub relative_time: String,
    /// Permission mode active when this message was created.
    pub permission_mode: Option<PermissionMode>,
    /// Per-row file metadata derived from transcript tool results
    /// between this user message and the next selectable user message.
    /// It renders only after async `can_restore_code` confirms that a
    /// restorable snapshot exists. TS: `computeDiffStatsBetweenMessages`
    /// inside the `fileHistoryMetadata` map (`MessageSelector.tsx:285-312`).
    pub diff_stats: Option<RewindDiffStatsPayload>,
    /// Whether file-history can restore this message at all (snapshot
    /// exists). TS: `fileHistoryCanRestore` returning false renders
    /// "⚠ No code restore". `None` = unknown / still loading.
    pub can_restore_code: Option<bool>,
}

impl RewindableMessage {
    /// True for the synthetic last row that anchors the default
    /// selection to "now". Selecting it dispatches no rewind —
    /// equivalent to pressing Esc. TS: virtual `currentUUID`
    /// user-message in `MessageSelector.tsx:60-66`, rendered as
    /// `(current)` italic at line 591-601.
    pub fn is_synthetic(&self) -> bool {
        self.message_id.is_none()
    }
}

/// Rewind state.
///
/// TS: MessageSelector component state (selectedIndex, messageToRestore,
/// selectedRestoreOption, diffStatsForRestore, etc.)
#[derive(Debug, Clone)]
pub struct RewindState {
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
    pub diff_stats: Option<RewindDiffStatsPayload>,
    /// Message UUID that [`Self::diff_stats`] belongs to. Row metadata
    /// and restore preview intentionally have different lifetimes:
    /// TS computes per-row metadata between adjacent user turns, then
    /// loads restore stats for the selected checkpoint separately.
    pub diff_stats_message_id: Option<uuid::Uuid>,
    /// Whether file history is enabled for this session.
    pub file_history_enabled: bool,
    /// Whether file history has changes for selected message.
    pub has_file_changes: bool,
    /// Whether the `SummarizeUpTo` option is shown in the picker.
    /// TS: gated by `'external' === 'ant'`; we surface it via the
    /// `rewind.allow_summarize_up_to` setting (default false).
    pub allow_summarize_up_to: bool,
    /// Captured user feedback when the picker is in the
    /// SummarizeFeedback phase. None until the user types something.
    pub summarize_feedback: String,
    /// Pending summarize direction (carried from RestoreOptions to
    /// SummarizeFeedback so Enter on the feedback line knows whether
    /// to dispatch SummarizeFrom or SummarizeUpTo).
    pub pending_summarize: Option<SummarizeDirection>,
    /// True when the picker was opened pre-anchored to a specific
    /// message (skipping the message-select phase). TS:
    /// `preselectedMessage` (`MessageSelector.tsx:42-44`). Esc dismisses
    /// fully instead of stepping back to the message list since there
    /// is no list to step back into.
    pub preselected: bool,
}

/// Direction selector for partial-compact rewind options.
///
/// TS: `direction = 'from' | 'up_to'` in `MessageSelector.tsx:195`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummarizeDirection {
    From,
    UpTo,
}

/// Build available restore options based on file history state.
///
/// TS: getRestoreOptions(canRestoreCode) in MessageSelector.tsx
/// (lines 93-134). Summarize is always offered; SummarizeUpTo is
/// gated behind `allow_summarize_up_to` to mirror TS's Anthropic-only
/// flag.
pub fn build_restore_options(
    file_history_enabled: bool,
    has_file_changes: bool,
    allow_summarize_up_to: bool,
) -> Vec<RestoreType> {
    let mut opts = if file_history_enabled && has_file_changes {
        vec![
            RestoreType::Both,
            RestoreType::ConversationOnly,
            RestoreType::CodeOnly,
        ]
    } else {
        vec![RestoreType::ConversationOnly]
    };
    opts.push(RestoreType::SummarizeFrom { feedback: None });
    if allow_summarize_up_to {
        opts.push(RestoreType::SummarizeUpTo { feedback: None });
    }
    // TS appends `nevermind` last (`MessageSelector.tsx:129-132`).
    // Selecting it cancels back to MessageSelect — same behavior as
    // pressing Esc, but explicitly listed in the picker so the
    // affordance is discoverable.
    opts.push(RestoreType::Nevermind);
    opts
}

#[cfg(test)]
#[path = "rewind.test.rs"]
mod tests;
