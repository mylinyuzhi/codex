//! User commands from TUI to core engine.
//!
//! These are the outbound messages sent from the TUI to the agent loop
//! when the user takes an action that requires core processing.

use coco_types::PermissionMode;
use coco_types::PermissionUpdate;

/// Which parts of the session to wipe on `/clear`.
///
/// TS: `clearConversation({ removeTasks, removeHooks, ... })` — we
/// collapse the per-subsystem flags into three user-visible scopes that
/// match the slash command variants. `All` is the most aggressive
/// (touches plan state + slug cache + regenerates session id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearScope {
    /// `/clear` — drop conversation transcript, keep session alive for
    /// `/resume`, keep plan mode state, keep model usage.
    Conversation,
    /// `/clear history` — alias of `Conversation` today; kept as a
    /// distinct variant so future refinement can diverge.
    History,
    /// `/clear all` — drop everything session-scoped (transcript +
    /// plan files + slug cache + plan-mode flags on app_state).
    All,
}

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
    /// Set permission mode. Replaces the legacy `SetPlanMode { bool }`
    /// — plan-mode activation is just `SetPermissionMode { mode: Plan }`.
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
    /// Clear conversation state — TUI has already wiped its local
    /// transcript; this tells the engine to reset its matching
    /// in-process state (plan-mode flags, attachment counters, slug
    /// cache) so the next turn starts clean. TS: `clearConversation()`.
    ClearConversation { scope: ClearScope },
    /// Shutdown the application.
    Shutdown,
}
