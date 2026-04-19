//! UI state — local TUI state, never sent to the agent.
//!
//! Overlay types live in `state::overlay`. This file keeps only the UiState
//! plus the pieces tightly coupled to it: input + history, focus, streaming,
//! and toasts.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use crate::constants;
use crate::state::overlay::Overlay;
use crate::theme::Theme;
use crate::widgets::suggestion_popup::SuggestionItem;

/// UI-only local state.
#[derive(Debug)]
pub struct UiState {
    /// Multi-line input state.
    pub input: InputState,
    /// Paste pill manager for tracking pasted content (text and images).
    pub paste_manager: crate::paste::PasteManager,
    /// Chat scroll offset (lines from bottom).
    pub scroll_offset: i32,
    /// Current focus target.
    pub focus: FocusTarget,
    /// Active modal overlay.
    pub overlay: Option<Overlay>,
    /// Queued overlays awaiting display.
    pub overlay_queue: VecDeque<Overlay>,
    /// Active streaming content.
    pub streaming: Option<StreamingState>,
    /// Whether thinking content is visible.
    pub show_thinking: bool,
    /// Whether system reminders are visible (debug).
    pub show_system_reminders: bool,
    /// Whether user has manually scrolled.
    pub user_scrolled: bool,
    /// Current theme.
    pub theme: Theme,
    /// Active toast notifications.
    pub toasts: VecDeque<Toast>,
    /// IDs of collapsed tool calls.
    pub collapsed_tools: HashSet<String>,
    /// Help overlay scroll position.
    pub help_scroll: i32,
    /// Kill ring for Ctrl+K / Ctrl+Y.
    pub kill_ring: String,
    /// Timestamp of last Esc press (for double-Esc rewind detection).
    pub last_esc_time: Option<Instant>,
    /// Whether the terminal window currently has focus. Used to gate
    /// turn-complete notifications so they only fire when the user has
    /// switched away — matches TS `ink::focus` semantics.
    pub terminal_focused: bool,
    /// Platform clipboard lease held alive for the lifetime of the TUI. On
    /// Linux/X11 and some Wayland setups the clipboard is served by the
    /// process that wrote it, so dropping the `arboard::Clipboard` handle
    /// would wipe the copied text. The lease is `None` on other platforms
    /// and on the OSC 52 path where no in-process ownership is required.
    pub clipboard_lease: Option<crate::clipboard_copy::ClipboardLease>,
    /// Active autocomplete suggestions (slash commands, @-mentions, etc.).
    /// `Some(_)` drives the keybinding bridge into `Autocomplete` context
    /// and renders a popup above the input. Recomputed after every input
    /// mutation in `autocomplete::refresh_suggestions`.
    pub active_suggestions: Option<ActiveSuggestions>,
}

impl UiState {
    /// Create a new default UI state.
    pub fn new() -> Self {
        Self {
            input: InputState::new(),
            paste_manager: crate::paste::PasteManager::new(),
            scroll_offset: 0,
            focus: FocusTarget::Input,
            overlay: None,
            overlay_queue: VecDeque::new(),
            streaming: None,
            show_thinking: true,
            show_system_reminders: false,
            user_scrolled: false,
            theme: Theme::default(),
            toasts: VecDeque::new(),
            collapsed_tools: HashSet::new(),
            help_scroll: 0,
            kill_ring: String::new(),
            last_esc_time: None,
            terminal_focused: true,
            clipboard_lease: None,
            active_suggestions: None,
        }
    }

    /// Set the active overlay using the [`Overlay::priority`] ranking.
    ///
    /// Rules (see `crate-coco-tui.md` §Overlay Priority):
    /// - No active overlay: install directly.
    /// - New overlay has strictly higher priority (lower number): displace
    ///   the current overlay back into the queue and install the new one.
    /// - Otherwise: insert into the queue at its priority position. Same
    ///   priority keeps insertion order (stable within a tier).
    ///
    /// Queue overflow drops the lowest-priority tail entry to make room so a
    /// security-critical overlay can still enqueue.
    pub fn set_overlay(&mut self, overlay: Overlay) {
        match self.overlay.take() {
            None => {
                self.overlay = Some(overlay);
            }
            Some(current) => {
                if overlay.priority() < current.priority() {
                    // New overlay has higher priority — displace current.
                    self.overlay = Some(overlay);
                    self.enqueue_overlay(current);
                } else {
                    // Same-or-lower priority: keep current, queue the new one.
                    self.overlay = Some(current);
                    self.enqueue_overlay(overlay);
                }
            }
        }
    }

    /// Insert `overlay` into the priority-ordered queue. Drops the
    /// lowest-priority entry on overflow to keep more important overlays in.
    fn enqueue_overlay(&mut self, overlay: Overlay) {
        let max = constants::MAX_OVERLAY_QUEUE as usize;
        let prio = overlay.priority();
        let pos = self
            .overlay_queue
            .iter()
            .position(|o| o.priority() > prio)
            .unwrap_or(self.overlay_queue.len());
        self.overlay_queue.insert(pos, overlay);
        while self.overlay_queue.len() > max {
            self.overlay_queue.pop_back();
        }
    }

    /// Dismiss the current overlay and show the next queued one.
    pub fn dismiss_overlay(&mut self) {
        self.overlay = self.overlay_queue.pop_front();
    }

    /// Whether there are active toasts.
    pub fn has_toasts(&self) -> bool {
        !self.toasts.is_empty()
    }

    /// Add a toast notification.
    pub fn add_toast(&mut self, toast: Toast) {
        if self.toasts.len() >= constants::MAX_TOASTS as usize {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    /// Remove expired toasts.
    pub fn expire_toasts(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

/// Current focus target in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusTarget {
    #[default]
    Input,
    Chat,
}

/// Which autocomplete trigger produced the active suggestions.
///
/// Determines the popup title, the source of suggestion items, and how the
/// accepted item is substituted back into the input. Kinds map to TS's four
/// mention triggers plus the leading `/` slash-command trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    /// Leading `/` — populated from `session.available_commands`.
    SlashCommand,
    /// `@path` — populated asynchronously by `FileSearchManager`.
    File,
    /// `@agent-name` — populated from the session's agent registry.
    Agent,
    /// `@#symbol` — populated asynchronously by `SymbolSearchManager` (LSP).
    Symbol,
}

impl SuggestionKind {
    /// Popup title shown at the top of the suggestion list.
    pub fn title(self) -> &'static str {
        match self {
            Self::SlashCommand => "Commands",
            Self::File => "Files",
            Self::Agent => "Agents",
            Self::Symbol => "Symbols",
        }
    }
}

/// Active autocomplete session: popup rendered above input, intercepts
/// `Up/Down/Tab/Esc` while letting regular typing pass through.
#[derive(Debug, Clone)]
pub struct ActiveSuggestions {
    pub kind: SuggestionKind,
    /// Items to show in the popup — filtered by `query` before display.
    pub items: Vec<SuggestionItem>,
    /// Currently selected index into the filtered list.
    pub selected: i32,
    /// The filter text the user has typed after the trigger.
    pub query: String,
    /// Character offset in `input.text` where the trigger started (the `/`
    /// or `@`). Used when accepting a suggestion to splice the selection
    /// back into the input.
    pub trigger_pos: i32,
}

/// A single history entry with frecency metadata.
///
/// TS: `frequencyMap` in PromptInput.tsx — each entry tracks how often it was
/// used and when it was last entered. Navigation sorts by a frecency score
/// (`ln(frequency) * recency_factor`) rather than raw insertion order, so a
/// command typed ten times last week floats above a one-off from yesterday.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub text: String,
    /// How many times this exact text has been submitted.
    pub frequency: i32,
    /// Unix-epoch seconds of the most recent submission.
    pub last_used_secs: i64,
}

impl HistoryEntry {
    /// Frecency score — higher is more relevant. Returns `f64` so the caller
    /// can sort_by on the raw value without losing ordering granularity.
    ///
    /// Formula: `ln(frequency + 1) * recency_factor`, where
    /// `recency_factor = 1.0` for entries less than 24h old and decays
    /// exponentially with 7-day half-life for older entries. This matches
    /// the TS weighting and guards `ln(0)` by adding 1 to frequency.
    pub fn frecency(&self, now_secs: i64) -> f64 {
        let freq = ((self.frequency.max(0) + 1) as f64).ln();
        let age = (now_secs - self.last_used_secs).max(0) as f64;
        let day = 86_400.0_f64;
        let recency = if age < day {
            1.0
        } else {
            let weeks = (age - day) / (7.0 * day);
            0.5_f64.powf(weeks)
        };
        freq * recency
    }
}

/// Multi-line input state.
#[derive(Debug)]
pub struct InputState {
    /// Current text content.
    pub text: String,
    /// Cursor position (character index, NOT byte).
    pub cursor: i32,
    /// Command history ordered by frecency (most-relevant first).
    pub history: Vec<HistoryEntry>,
    /// Current history navigation index.
    pub history_index: Option<i32>,
}

impl InputState {
    /// Create empty input.
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
        }
    }

    /// Insert a character at cursor.
    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.char_to_byte(self.cursor);
        self.text.insert(byte_pos, c);
        self.cursor += 1;
    }

    /// Delete character before cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let byte_pos = self.char_to_byte(self.cursor);
            let next_byte = self.char_to_byte(self.cursor + 1);
            self.text.replace_range(byte_pos..next_byte, "");
        }
    }

    /// Delete character at cursor.
    pub fn delete_forward(&mut self) {
        let len = self.text.chars().count() as i32;
        if self.cursor < len {
            let byte_pos = self.char_to_byte(self.cursor);
            let next_byte = self.char_to_byte(self.cursor + 1);
            self.text.replace_range(byte_pos..next_byte, "");
        }
    }

    /// Take the current input, clearing the buffer.
    pub fn take_input(&mut self) -> String {
        self.cursor = 0;
        self.history_index = None;
        std::mem::take(&mut self.text)
    }

    /// Move cursor left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right.
    pub fn cursor_right(&mut self) {
        let len = self.text.chars().count() as i32;
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    /// Move cursor to start of line.
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end of line.
    pub fn cursor_end(&mut self) {
        self.cursor = self.text.chars().count() as i32;
    }

    /// Whether the input is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Record a submitted text into history using frecency scoring.
    ///
    /// If the text already exists, bump its frequency and update `last_used`.
    /// Otherwise append a new entry. After insertion the vector is sorted
    /// descending by [`HistoryEntry::frecency`] so that up-arrow navigation
    /// walks the most relevant entries first. Capped at
    /// `constants::MAX_HISTORY_ENTRIES` by dropping the lowest-scoring tail.
    pub fn add_to_history(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        let now = now_unix_secs();
        if let Some(entry) = self.history.iter_mut().find(|h| h.text == text) {
            entry.frequency = entry.frequency.saturating_add(1);
            entry.last_used_secs = now;
        } else {
            self.history.push(HistoryEntry {
                text,
                frequency: 1,
                last_used_secs: now,
            });
        }
        // Sort by frecency desc; ties keep recent-first (stable sort on
        // original order where the most-recent append naturally sits last).
        self.history
            .sort_by(|a, b| b.frecency(now).total_cmp(&a.frecency(now)));
        let max = constants::MAX_HISTORY_ENTRIES as usize;
        if self.history.len() > max {
            self.history.truncate(max);
        }
    }

    /// Convert character index to byte index.
    fn char_to_byte(&self, char_idx: i32) -> usize {
        self.text
            .char_indices()
            .nth(char_idx as usize)
            .map(|(i, _)| i)
            .unwrap_or(self.text.len())
    }
}

/// Unix-epoch seconds for the frecency timestamp. Clock skew or a pre-epoch
/// system date falls back to 0, which dampens the recency factor but keeps
/// the history useful rather than panicking.
fn now_unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming display state.
#[derive(Debug, Clone)]
pub struct StreamingState {
    /// Accumulated text content.
    pub content: String,
    /// Accumulated thinking content.
    pub thinking: String,
    /// Current streaming mode.
    pub mode: StreamMode,
    /// Display cursor position for adaptive pacing.
    pub display_cursor: usize,
}

/// Current streaming content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamMode {
    Text,
    Thinking,
    ToolUse,
}

impl StreamingState {
    /// Create a new streaming state.
    pub fn new() -> Self {
        Self {
            content: String::new(),
            thinking: String::new(),
            mode: StreamMode::Text,
            display_cursor: 0,
        }
    }

    /// Append text delta.
    pub fn append_text(&mut self, delta: &str) {
        self.content.push_str(delta);
        self.mode = StreamMode::Text;
    }

    /// Append thinking delta.
    pub fn append_thinking(&mut self, delta: &str) {
        self.thinking.push_str(delta);
        self.mode = StreamMode::Thinking;
    }

    /// Get visible content up to display cursor.
    pub fn visible_content(&self) -> &str {
        let end = self.display_cursor.min(self.content.len());
        &self.content[..end]
    }

    /// Advance display cursor (returns true if changed).
    pub fn advance_display(&mut self) -> bool {
        if self.display_cursor < self.content.len() {
            // Advance by one line or to end
            let remaining = &self.content[self.display_cursor..];
            let advance = remaining
                .find('\n')
                .map(|i| i + 1)
                .unwrap_or(remaining.len());
            self.display_cursor += advance;
            true
        } else {
            false
        }
    }

    /// Reveal all content immediately.
    pub fn reveal_all(&mut self) {
        self.display_cursor = self.content.len();
    }
}

impl Default for StreamingState {
    fn default() -> Self {
        Self::new()
    }
}

/// Toast notification.
#[derive(Debug, Clone)]
pub struct Toast {
    pub message: String,
    pub severity: ToastSeverity,
    pub created_at: Instant,
    pub duration: Duration,
}

/// Toast severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastSeverity {
    Info,
    Success,
    Warning,
    Error,
}

impl Toast {
    /// Create an info toast.
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Info,
            created_at: Instant::now(),
            duration: constants::TOAST_INFO_DURATION,
        }
    }

    /// Create a success toast.
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Success,
            created_at: Instant::now(),
            duration: constants::TOAST_SUCCESS_DURATION,
        }
    }

    /// Create a warning toast.
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Warning,
            created_at: Instant::now(),
            duration: constants::TOAST_WARNING_DURATION,
        }
    }

    /// Create an error toast.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            severity: ToastSeverity::Error,
            created_at: Instant::now(),
            duration: constants::TOAST_ERROR_DURATION,
        }
    }

    /// Whether the toast has expired.
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }

    /// Remaining percentage (1.0 = full, 0.0 = expired).
    pub fn remaining_pct(&self) -> f64 {
        let elapsed = self.created_at.elapsed().as_secs_f64();
        let total = self.duration.as_secs_f64();
        (1.0 - elapsed / total).max(0.0)
    }
}
