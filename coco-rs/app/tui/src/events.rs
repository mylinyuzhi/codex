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
    /// Show doctor/diagnostics.
    ShowDoctor,
    /// Show the tabbed settings panel.
    ShowSettings,
    /// Switch to the next Settings tab (Tab in settings overlay).
    SettingsNextTab,
    /// Switch to the previous Settings tab (Shift+Tab in settings overlay).
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
    /// Confirm selection in overlay.
    OverlayConfirm,

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
    /// Quit the application.
    Quit,
}
