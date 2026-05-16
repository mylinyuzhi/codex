//! TUI events and commands — the Message layer in TEA.
//!
//! - [`TuiEvent`]: low-level events from terminal, agent, and timers
//! - [`TuiCommand`]: high-level user actions derived from events

use crossterm::event::KeyEvent;

/// Low-level TUI events from all sources.
///
/// The main event loop receives these via `tokio::select!` multiplexing.
///
/// Note: no mouse events. coco-tui deliberately leaves mouse input to the
/// terminal emulator so the native drag-to-select / Cmd+C flow keeps working
/// (same choice as codex-rs). `terminal.rs` never calls `EnableMouseCapture`.
#[derive(Debug)]
pub enum TuiEvent {
    // ── Terminal events ──
    /// Keyboard input.
    Key(KeyEvent),
    /// Terminal resized.
    Resize { width: u16, height: u16 },
    /// Terminal focus changed.
    FocusChanged { focused: bool },

    // ── Timing events ──
    /// Redraw request.
    Draw,
    /// Status tick (250ms) — toast expiry, idle detection.
    Tick,
    /// Spinner tick (50ms) — animation frames.
    SpinnerTick,

    // ── Input events ──
    /// Bracketed paste content.
    Paste(String),

    // ── Process control ──
    /// User requested process suspend (Ctrl+Z on Unix).
    ///
    /// In raw mode the terminal no longer translates Ctrl+Z into a
    /// SIGTSTP automatically, so [`App::convert_crossterm_event`]
    /// intercepts the key and emits this event. The handler calls
    /// `Tui::trigger_suspend`, which delivers `SIGTSTP` to the process
    /// group via `libc::kill` and blocks until SIGCONT.
    ///
    /// No-op on non-Unix platforms (key falls through as a normal
    /// `Key` event).
    Suspend,

    // ── Permission events ──
    /// Background classifier approved a pending permission request.
    ///
    /// TS: interactiveHandler.ts races classifier with user input.
    /// When the classifier approves before the user responds, auto-dismiss
    /// the overlay with this event.
    ClassifierApproved {
        request_id: String,
        /// The matched rule name from the classifier.
        matched_rule: Option<String>,
    },
    /// Background classifier denied or was unavailable.
    ClassifierDenied { request_id: String, reason: String },
}

/// High-level user commands derived from keyboard/event processing.
///
/// Each variant maps to a state mutation in `update.rs::handle_command()`.
#[derive(Debug, Clone)]
pub enum TuiCommand {
    // ── Mode toggles ──
    /// Toggle plan mode.
    TogglePlanMode,
    /// Cycle thinking level (off → low → medium → high).
    CycleThinkingLevel,
    /// Toggle thinking content visibility.
    ToggleThinking,
    /// Cycle model (show model picker).
    CycleModel,
    /// Cycle permission mode.
    CyclePermissionMode,
    /// Toggle fast mode.
    ToggleFastMode,

    // ── Input actions ──
    /// Submit current input (or queue if streaming).
    SubmitInput,
    /// Queue input during streaming.
    QueueInput,
    /// Interrupt current operation.
    Interrupt,
    /// Cancel current action / close overlay.
    Cancel,
    /// Clear screen.
    ClearScreen,

    // ── Text editing ──
    /// Insert a character at cursor.
    InsertChar(char),
    /// Insert a newline.
    InsertNewline,
    /// Delete character before cursor.
    DeleteBackward,
    /// Delete character at cursor.
    DeleteForward,
    /// Delete word before cursor.
    DeleteWordBackward,
    /// Delete word after cursor.
    DeleteWordForward,
    /// Kill to end of line (Ctrl+K).
    KillToEndOfLine,
    /// Kill from beginning of line to cursor (Ctrl+U).
    KillToBeginningOfLine,
    /// Yank killed text (Ctrl+Y).
    Yank,

    // ── Cursor movement ──
    /// Move cursor left.
    CursorLeft,
    /// Move cursor right.
    CursorRight,
    /// Move cursor up (history previous).
    CursorUp,
    /// Move cursor down (history next).
    CursorDown,
    /// Move cursor to start of line.
    CursorHome,
    /// Move cursor to end of line.
    CursorEnd,
    /// Move cursor one word left.
    WordLeft,
    /// Move cursor one word right.
    WordRight,

    // ── Scrolling ──
    /// Scroll up by line step.
    ScrollUp,
    /// Scroll down by line step.
    ScrollDown,
    /// Scroll up by page.
    PageUp,
    /// Scroll down by page.
    PageDown,

    // ── Focus ──
    /// Focus next panel.
    FocusNext,
    /// Focus previous panel.
    FocusPrevious,
    /// Focus next agent in side panel.
    FocusNextAgent,
    /// Focus previous agent in side panel.
    FocusPrevAgent,

    // ── Overlay actions ──
    /// Approve (Y in permission dialog).
    Approve,
    /// Deny (N in permission dialog).
    Deny,
    /// Approve all / always allow (A in permission dialog).
    ApproveAll,
    /// Classifier auto-approved a pending permission.
    /// TS: interactiveHandler.ts onAllow from classifier path.
    ClassifierAutoApprove {
        request_id: String,
        matched_rule: Option<String>,
    },

    // ── Commands & overlays ──
    /// Execute a skill.
    ExecuteSkill(String),
    /// Execute a slash command by name (no leading `/`). Triggered by
    /// a `command:foo` keybinding from the user's `keybindings.json`
    /// — fires through the same handler as if the user typed `/foo`
    /// and hit Enter.
    ExecuteSlashCommand(String),
    /// Show help overlay.
    ShowHelp,
    /// Show command palette overlay.
    ShowCommandPalette,
    /// Show session browser overlay.
    ShowSessionBrowser,
    /// Show global search (Ctrl+Shift+F).
    ShowGlobalSearch,
    /// Show quick open (Ctrl+O).
    ShowQuickOpen,
    /// Show export dialog.
    ShowExport,
    /// Show context visualization.
    ShowContextViz,
    /// Show rewind overlay (message selector).
    /// TS: triggered by double-Esc or /rewind command.
    ShowRewind,
    /// Open the rewind overlay pre-anchored to a specific message,
    /// jumping straight to the RestoreOptions confirm screen. TS:
    /// `preselectedMessage` flow (`MessageSelector.tsx:42-44`). Used
    /// by message-actions `edit` and by the non-lossless branch of
    /// auto-restore-on-interrupt.
    ShowRewindFor { message_id: String },
    /// Show doctor/diagnostics.
    ShowDoctor,
    /// Show the tabbed settings panel.
    ShowSettings,
    /// Toggle language-level syntax highlighting for markdown code blocks.
    ToggleSyntaxHighlighting,
    /// Tab: cycle Settings tab forward (Settings overlay) OR cycle
    /// question/footer focus (Question overlay). Handler in update.rs
    /// dispatches per-overlay. TS Question parity:
    /// `handleTabNext` in `AskUserQuestionPermissionRequest.tsx`.
    SettingsNextTab,
    /// Shift+Tab variant of [`SettingsNextTab`].
    SettingsPrevTab,

    // ── Overlay navigation ──
    /// Filter text in active filterable overlay.
    OverlayFilter(char),
    /// Delete char from overlay filter.
    OverlayFilterBackspace,
    /// Select next item in overlay list.
    OverlayNext,
    /// Select previous item in overlay list.
    OverlayPrev,
    /// Jump selection to the first item in the overlay list. TS:
    /// `messageSelector:top` (`Home` / `Shift+Up` / `Meta+Up` / `Shift+K`).
    OverlayJumpStart,
    /// Jump selection to the last item in the overlay list. TS:
    /// `messageSelector:bottom` (`End` / `Shift+Down` / `Meta+Down` / `Shift+J`).
    OverlayJumpEnd,
    /// Confirm selection in overlay.
    OverlayConfirm,
    /// Cycle thinking effort in the ModelPicker overlay by `delta`.
    /// Bound to Left/Right via `modelPicker:decreaseEffort` /
    /// `modelPicker:increaseEffort`. Distinct from `OverlayPrev/Next`
    /// (Up/Down) because the picker has two orthogonal cursors —
    /// model row and effort level — TS solves this with the same
    /// `←/→` axis (`useEffortNavigation`).
    ModelPickerCycleEffort(i32),
    /// Cycle the configured role in the ModelPicker overlay by `delta`.
    /// Tab → +1, Shift+Tab → -1. coco-rs-only extension to the TS
    /// picker (TS only ever drives the `main` model).
    ModelPickerCycleRole(i32),

    // ── Task management ──
    /// Background all foreground tasks.
    BackgroundAllTasks,
    /// Kill all running agents.
    KillAllAgents,

    // ── External editor ──
    /// Open input in external editor ($EDITOR).
    OpenExternalEditor,
    /// Open plan file in external editor.
    OpenPlanEditor,

    // ── Display toggles ──
    /// Toggle tool call collapse.
    ToggleToolCollapse,
    /// Toggle system reminder visibility.
    ToggleSystemReminders,

    // ── Clipboard ──
    /// Paste from clipboard (image first, text fallback).
    PasteFromClipboard,
    /// Copy the last agent response to the system clipboard (Ctrl+O / /copy).
    /// Mirrors codex-rs's `ChatWidget::copy_last_agent_markdown`.
    CopyLastMessage,

    // ── Application ──
    /// Quit the application **immediately**. Issued by `/quit` /
    /// `/exit` slash commands and by `exit::ExitEffect::Quit`
    /// (delivered after a double-press has confirmed the user's intent).
    /// Plain Ctrl+C / Ctrl+D do NOT emit `Quit` directly — they go
    /// through [`RequestExit`] or [`Interrupt`] so the double-press
    /// state machine in `update::exit` gets a chance to run.
    Quit,
    /// User pressed an exit key (Ctrl+D in defaults). The handler in
    /// `update::exit::on_request_exit` runs the double-press tracker
    /// and either arms the "Press X again to exit" hint or fires
    /// [`Quit`]. Distinct from [`Interrupt`] (Ctrl+C) because Ctrl+D
    /// has no "cancel running task" semantics — its first press only
    /// arms the prompt.
    RequestExit,

    // ── Stash ──
    /// Push to / pop from the input-draft stash slot. TS
    /// `chat:stash` semantics from
    /// `PromptInput.tsx::handleStash`: empty input + stash present
    /// → pop; non-empty input → push (overwrites).
    StashInputDraft,

    // ── Expanded view ──
    /// Cycle the right-rail `expanded_view` between `None`, `Tasks`,
    /// and (when teammates are running) `Teammates`. TS
    /// `app:toggleTodos` (`useGlobalKeybindings.tsx::handleToggleTodos`).
    ToggleExpandedTasksView,
    /// Toggle whether teammate spinner lines show recent message
    /// preview text. TS `app:toggleTeammatePreview`
    /// (`AppStateStore.ts::showTeammateMessagePreview`).
    ToggleTeammateMessagePreview,
    /// Open / close the transcript overlay. TS `app:toggleTranscript`
    /// (`useGlobalKeybindings.tsx::handleToggleTranscript`) — verbose,
    /// scrollable, all-messages view.
    ToggleTranscript,
    /// Toggle `show_all` on the active transcript overlay (include /
    /// hide meta messages). TS `transcript:toggleShowAll`
    /// (`Ctrl+E` while the transcript screen is mounted). No-op when
    /// the active overlay is not a transcript.
    ToggleTranscriptShowAll,
}
