//! User commands from TUI to core engine.
//!
//! These are the outbound messages sent from the TUI to the agent loop
//! when the user takes an action that requires core processing.

use coco_types::PermissionMode;
use coco_types::PermissionUpdate;

/// Commands sent from TUI to the core agent loop.
#[derive(Debug, Clone)]
pub enum UserCommand {
    /// Submit user input text with resolved paste data.
    SubmitInput {
        /// Resolved text content (paste pills expanded, image pills removed).
        content: String,
        /// Original input text (with pills intact) for display in chat history.
        display_text: Option<String>,
        /// Image data from pasted images (clipboard or drag-drop).
        images: Vec<crate::paste::ImageData>,
    },
    /// Interrupt current operation (Ctrl+C).
    Interrupt,
    /// Set plan mode active/inactive.
    SetPlanMode { active: bool },
    /// Set permission mode.
    SetPermissionMode { mode: PermissionMode },
    /// Set thinking level.
    SetThinkingLevel { level: String },
    /// Set model.
    SetModel { model: String },
    /// Respond to a permission prompt.
    ///
    /// TS: `onAllow(updatedInput, permissionUpdates, feedback, contentBlocks)`
    /// and `onReject(feedback, contentBlocks)`.
    ApprovalResponse {
        request_id: String,
        approved: bool,
        always_allow: bool,
        /// User feedback explaining their decision (why they approved/denied).
        /// TS: `acceptFeedback` / `rejectFeedback`
        feedback: Option<String>,
        /// Modified tool input (user edited the command/path before approving).
        /// TS: `updatedInput`
        updated_input: Option<serde_json::Value>,
        /// Permission rules to persist from this decision.
        /// TS: `permissionUpdates` (suggestions the user accepted)
        permission_updates: Vec<PermissionUpdate>,
    },
    /// Execute a skill by name.
    ExecuteSkill { name: String, args: Option<String> },
    /// Queue a command for mid-turn injection.
    QueueCommand { prompt: String },
    /// Background all foreground tasks.
    BackgroundAllTasks,
    /// Kill all running agents.
    KillAllAgents,
    /// Toggle fast mode.
    ToggleFastMode,
    /// Trigger manual compaction.
    Compact,
    /// Rewind to a previous checkpoint.
    /// TS: rewindConversationTo() + fileHistoryRewind() in REPL.tsx
    Rewind {
        message_id: String,
        restore_type: crate::state::rewind::RestoreType,
    },
    /// Request diff stats for a message (async, response via ServerNotification).
    /// TS: fileHistoryGetDiffStats() called from MessageSelector useEffect.
    RequestDiffStats { message_id: String },
    /// Shutdown the application.
    Shutdown,
}
