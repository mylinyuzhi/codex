//! User commands from TUI to core engine.
//!
//! These are the outbound messages sent from the TUI to the agent loop
//! when the user takes an action that requires core processing.

use std::fmt;

use coco_types::PermissionMode;
use coco_types::PermissionUpdate;

/// Which parts of the session to wipe on `/clear`.
///
/// TS reference: `commands/clear/conversation.ts::clearConversation` is a
/// single function with no scope parameter; `/clear` always performs the
/// full reset (transcript + caches + session-id regen + plan slugs +
/// file history + SessionEnd/SessionStart hooks). coco-rs preserves
/// `/clear` (and the `/clear all` alias) at TS parity, then adds
/// `/clear history` as a Rust-only lighter scope for users who want to
/// declutter the transcript without disturbing tools / file caches /
/// plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearScope {
    /// `/clear` — TS-aligned full reset: transcript, FileReadState,
    /// FileHistoryState, plan-slug cache, session-memory baseline,
    /// cache-break detector. Plan-mode app_state flags reset to
    /// default. Sets up the next turn as a fresh session.
    Conversation,
    /// `/clear history` — Rust-only lighter scope: transcript +
    /// plan-mode app_state reset only. Tools, file caches, plans, and
    /// session-memory baseline are preserved — useful for "I just want
    /// the screen back" without invalidating any work.
    History,
    /// `/clear all` — alias of [`Self::Conversation`]. Retained for
    /// users who already typed it; behaves identically to `/clear`.
    All,
}

/// Why the TUI requested process shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownReason {
    /// User typed `/exit` or `/quit`.
    SlashCommand,
    /// User invoked an immediate-quit command such as Ctrl+Q.
    ImmediateQuit,
    /// User confirmed Ctrl+C double-press exit.
    DoublePressCtrlC,
    /// User confirmed Ctrl+D double-press exit.
    DoublePressCtrlD,
}

impl ShutdownReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlashCommand => "slash_command",
            Self::ImmediateQuit => "immediate_quit",
            Self::DoublePressCtrlC => "double_press_ctrl_c",
            Self::DoublePressCtrlD => "double_press_ctrl_d",
        }
    }
}

impl fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Commands sent from TUI to the core agent loop.
#[derive(Debug, Clone)]
pub enum UserCommand {
    /// Submit a bash-mode entry (input started with `!`). The TUI has
    /// already stripped the leading `!` and pushed a
    /// `ChatMessage::BashInput` locally; the engine bridge in
    /// `tui_runner` runs the command via `coco_shell::ShellExecutor`
    /// and emits a `ChatMessage::BashOutput` back through the
    /// `ServerNotification::Message` channel. TS parity:
    /// `LocalShellTask.tsx` — bypasses the model loop entirely.
    SubmitBash {
        /// User-message UUID minted at submit time so the BashInput
        /// and BashOutput messages can share a parent id for rewind.
        user_message_id: String,
        /// Shell command (already prefix-stripped).
        command: String,
    },
    /// Open a memory file chosen from the `/memory` picker. The TUI
    /// only owns selection state; the CLI bridge owns filesystem and
    /// process effects so terminal/editor behavior stays outside
    /// reducers and renderers.
    OpenMemoryFile {
        /// Memory file target selected by the picker.
        path: std::path::PathBuf,
    },
    /// Open the current prompt draft in an external editor. The TUI
    /// sends the current text; the CLI bridge owns temp-file and
    /// process effects, then emits a TUI event with the edited text.
    OpenPromptEditor {
        /// Prompt content to seed into the editor buffer.
        initial_content: String,
    },
    /// Open this session's plan file in an external editor. The CLI
    /// bridge resolves the concrete plan-file path from the current
    /// session id and runtime config before launching the editor.
    OpenPlanEditor,
    /// The TUI has left raw mode and any active overlay alt-screen, so
    /// the CLI runner may now start the editor process for `request_id`.
    ExternalEditorTerminalReady {
        /// Opaque id from `TuiOnlyEvent::ExternalEditorPrepare`.
        request_id: String,
    },
    /// The TUI failed to prepare terminal modes, so the CLI runner
    /// should drop the pending editor request and surface this failure.
    ExternalEditorTerminalPrepareFailed {
        /// Opaque id from `TuiOnlyEvent::ExternalEditorPrepare`.
        request_id: String,
        /// User-visible failure summary.
        error: String,
    },
    /// Submit user input text with resolved paste data.
    SubmitInput {
        /// User-message UUID minted at submit time. The TUI pushes a
        /// `ChatMessage` carrying this id, the agent driver builds the
        /// `Message::User` carrying the same id, and `FileHistoryState`
        /// keys the per-turn snapshot on it. Single source of truth so
        /// rewind picker selections, file-history snapshots, and the
        /// JSONL transcript line up.
        ///
        /// TS parity: `screens/REPL.tsx`'s `onSubmit` mints
        /// `randomUUID()` once via `createUserMessage()` before the
        /// engine sees the message.
        user_message_id: String,
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
    /// Set the Main role's thinking effort.
    ///
    /// Emitted by [`crate::events::TuiCommand::CycleThinkingLevel`]
    /// (Ctrl+T). `level` is the wire-form string from
    /// `ReasoningEffort::to_string` (e.g. `"high"`, `"xhigh"`). The
    /// engine resolves it in-memory via `SessionRuntime::apply_role_effort`
    /// — no file write.
    SetThinkingLevel { level: String },
    /// Set the model bound to `role` plus its thinking effort. Emitted
    /// by the role-pill model picker on Enter; the engine applies the
    /// selection in-memory via `SessionRuntime::apply_role_override`
    /// — no file write. Non-Main roles take effect on the next turn
    /// that drives that role; Main effort takes effect immediately,
    /// Main model_id changes require a session restart (v1 limitation).
    SetModelRole {
        role: coco_types::ModelRole,
        provider: String,
        model_id: String,
        /// Chosen effort. `None` when the model has no thinking capability.
        effort: Option<coco_types::ReasoningEffort>,
    },
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
        /// Optional content blocks (image attachments etc.) the user
        /// pasted alongside the answer. TS: `contentBlocks` — 4th arg
        /// of `onAllow` / 2nd arg of `onReject`. Today no TUI gesture
        /// emits this; SDK clients ship via `ApprovalResolveParams.content_blocks`.
        content_blocks: Option<Vec<serde_json::Value>>,
    },
    /// Execute a skill by name.
    ExecuteSkill { name: String, args: Option<String> },
    /// Queue a command for mid-turn injection.
    ///
    /// Sent by [`crate::update::QueueInput`] when the user presses
    /// Enter while the agent is streaming. The CLI bridge in
    /// `tui_runner` forwards this to
    /// `runtime.command_queue().enqueue(...)` so the engine sees the
    /// prompt at the next drain point. `images` carries any pasted
    /// images at submit time so mid-turn screenshot pastes survive
    /// queueing — same shape as [`Self::SubmitInput`].
    QueueCommand {
        prompt: String,
        images: Vec<crate::paste::ImageData>,
    },
    /// Background all foreground tasks.
    BackgroundAllTasks,
    /// Kill all running agents.
    KillAllAgents,
    /// Toggle fast mode.
    ToggleFastMode,
    /// Trigger manual compaction. Optional `custom_instructions` carry
    /// any text after `/compact` so the LLM summarizer prompt can honor
    /// the user's focus directive. TS: `commands/compact/compact.ts:40`
    /// passes `args.trim()` as `customInstructions`.
    Compact { custom_instructions: Option<String> },
    /// Rewind to a previous checkpoint.
    /// TS: rewindConversationTo() + fileHistoryRewind() in REPL.tsx
    Rewind {
        message_id: String,
        restore_type: crate::state::rewind::RestoreType,
        /// 1-based turn number the user picked, for the protocol-level
        /// `rewind/completed` notification. TS does not need this
        /// (renders on the React side); coco-rs threads it through so
        /// SDK consumers see it without a second query.
        rewound_turn: i32,
    },
    /// Request diff stats for a message (async, response via ServerNotification).
    /// TS: fileHistoryGetDiffStats() called from MessageSelector useEffect.
    RequestDiffStats { message_id: String },
    /// Clear conversation state — TUI has already wiped its local
    /// transcript; this tells the engine to reset its matching
    /// in-process state (plan-mode flags, attachment counters, slug
    /// cache) so the next turn starts clean. TS: `clearConversation()`.
    ClearConversation { scope: ClearScope },
    /// Team lead responding to a teammate's plan-approval request.
    /// The engine routes this to the teammate's mailbox as a
    /// `plan_approval_response` envelope. TS: the response side of
    /// `ExitPlanModeV2Tool.ts:137-141` request flow.
    PlanApprovalResponse {
        request_id: String,
        /// Teammate agent name to address the response envelope to —
        /// carried in from `PlanApprovalOverlay.from` so we don't have
        /// to re-scan mailbox state to correlate the request_id.
        teammate_agent: String,
        approved: bool,
        /// Optional feedback the leader attached (e.g. "good, but please
        /// add tests"). `None` when the user just approved/denied
        /// without typing anything.
        feedback: Option<String>,
    },
    /// Shutdown the application.
    Shutdown { reason: ShutdownReason },
    /// Fire an `idle_prompt` Notification hook. The TUI emits this
    /// once per turn-completion epoch when the user has been idle
    /// past the configured threshold. TS parity:
    /// `screens/REPL.tsx:3934-3937` (`sendNotification({
    /// notificationType: 'idle_prompt' })`). The runtime translates
    /// this into a `coco_hooks::orchestration::execute_notification`
    /// call so registered `Notification` hooks can react.
    FireIdleNotification { message: String },
}
