//! UI-specific state.
//!
//! This module contains state that is local to the TUI and not
//! synchronized with the agent.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::time::Duration;
use std::time::Instant;

use cocode_protocol::ApprovalRequest;
use cocode_protocol::RoleSelection;

use crate::theme::Theme;
use crate::widgets::Toast;

/// UI-specific state.
#[derive(Debug, Clone, Default)]
pub struct UiState {
    /// The current input state.
    pub input: InputState,

    /// Scroll offset in the chat history.
    pub scroll_offset: i32,

    /// Current focus target.
    pub focus: FocusTarget,

    /// Active overlay (modal dialog).
    pub overlay: Option<Overlay>,

    /// Queued overlays waiting to be displayed (lower priority than active overlay).
    ///
    /// When a permission or question overlay arrives while another overlay is
    /// active, it is queued here. When the active overlay is cleared, the
    /// highest-priority queued overlay is promoted.
    pub overlay_queue: VecDeque<Overlay>,

    /// Streaming content state.
    pub streaming: Option<StreamingState>,

    /// File autocomplete state (shown when typing @path).
    pub file_suggestions: Option<FileSuggestionState>,

    /// Skill autocomplete state (shown when typing /command).
    pub skill_suggestions: Option<SkillSuggestionState>,

    /// Agent autocomplete state (shown when typing @agent-*).
    pub agent_suggestions: Option<AgentSuggestionState>,

    /// Symbol autocomplete state (shown when typing @#symbol).
    pub symbol_suggestions: Option<SymbolSuggestionState>,

    /// Whether to show thinking content in chat messages.
    pub show_thinking: bool,

    /// Whether the user has manually scrolled (disables auto-scroll).
    pub user_scrolled: bool,

    /// When thinking started (for duration calculation).
    pub thinking_started_at: Option<Instant>,

    /// Duration of the last completed thinking phase.
    pub last_thinking_duration: Option<Duration>,

    /// Whether the terminal window is focused.
    pub terminal_focused: bool,

    /// Current theme.
    pub theme: Theme,

    /// Active toast notifications.
    pub toasts: VecDeque<Toast>,

    /// Animation frame counter (0-7 cycle) for animated elements.
    pub animation_frame: u8,

    /// Toast ID counter for generating unique IDs.
    toast_id_counter: i32,

    /// Set of tool call IDs that are collapsed in the chat view.
    pub collapsed_tools: HashSet<String>,

    /// Scroll offset for help overlay.
    pub help_scroll: i32,

    /// Timestamp of the last Esc keypress (for double-Esc detection).
    pub last_esc_time: Option<Instant>,

    /// Query timing tracker for slow-query notification and
    /// permission-pause exclusion.
    pub query_timing: QueryTiming,
}

impl UiState {
    /// Set the overlay, queuing it if a higher-priority overlay is active.
    ///
    /// Agent-driven overlays (permissions, questions, errors) are queued when
    /// another overlay is active. User-triggered overlays replace the current
    /// overlay, moving any displaced agent-driven overlay to the queue.
    /// Maximum queued overlays to prevent unbounded growth.
    const MAX_OVERLAY_QUEUE: usize = 16;

    /// Set the active overlay, queuing or displacing existing overlays as needed.
    pub fn set_overlay(&mut self, overlay: Overlay) {
        if let Some(ref current) = self.overlay {
            if overlay.is_agent_driven() && current.priority() <= overlay.priority() {
                // Current overlay has equal or higher priority — queue the new one
                if self.overlay_queue.len() < Self::MAX_OVERLAY_QUEUE {
                    self.overlay_queue.push_back(overlay);
                }
            } else if current.is_agent_driven() && !overlay.is_agent_driven() {
                // User-triggered overlay takes over; queue the displaced agent overlay
                if let Some(displaced) = self.overlay.take() {
                    self.overlay_queue.push_front(displaced);
                }
                self.overlay = Some(overlay);
            } else {
                // Higher-priority agent replaces current, or user-to-user replacement.
                // Queue displaced agent-driven overlay so it resurfaces later.
                if current.is_agent_driven()
                    && self.overlay_queue.len() < Self::MAX_OVERLAY_QUEUE
                    && let Some(displaced) = self.overlay.take()
                {
                    self.overlay_queue.push_back(displaced);
                }
                self.overlay = Some(overlay);
            }
        } else {
            self.overlay = Some(overlay);
        }
    }

    /// Clear the overlay and promote the next queued overlay (if any).
    ///
    /// If the cleared overlay was a permission dialog, resumes the query
    /// timing tracker (permission pause ends when user responds).
    pub fn clear_overlay(&mut self) {
        // Resume query timing if a blocking dialog was dismissed
        if matches!(
            self.overlay,
            Some(Overlay::Permission(_) | Overlay::PlanExitApproval(_) | Overlay::Question(_))
        ) {
            self.query_timing.on_permission_dialog_close();
        }
        self.overlay = None;
        // Promote the highest-priority queued overlay
        if !self.overlay_queue.is_empty() {
            let best_idx = self
                .overlay_queue
                .iter()
                .enumerate()
                .min_by_key(|(_, o)| o.priority())
                .map(|(i, _)| i);
            if let Some(idx) = best_idx {
                self.overlay = self.overlay_queue.remove(idx);
                // Re-pause timing if promoted overlay also blocks the user
                if matches!(
                    self.overlay,
                    Some(
                        Overlay::Permission(_)
                            | Overlay::PlanExitApproval(_)
                            | Overlay::Question(_)
                    )
                ) {
                    self.query_timing.on_permission_dialog_open();
                }
            }
        }
    }

    /// Get the number of queued overlays waiting behind the active one.
    pub fn queued_overlay_count(&self) -> i32 {
        self.overlay_queue.len() as i32
    }

    /// Start streaming.
    pub fn start_streaming(&mut self, turn_id: String) {
        self.streaming = Some(StreamingState::new(turn_id));
    }

    /// Stop streaming.
    pub fn stop_streaming(&mut self) {
        self.streaming = None;
    }

    /// Append to streaming content and transition mode to Responding.
    pub fn append_streaming(&mut self, delta: &str) {
        if let Some(ref mut streaming) = self.streaming {
            streaming.content.push_str(delta);
            streaming.mode = StreamMode::Responding;
        }
    }

    /// Append to streaming thinking content and transition mode to Thinking.
    pub fn append_streaming_thinking(&mut self, delta: &str) {
        if let Some(ref mut streaming) = self.streaming {
            streaming.thinking.push_str(delta);
            streaming.mode = StreamMode::Thinking;
        }
    }

    /// Get the current stream mode (if streaming).
    pub fn stream_mode(&self) -> Option<StreamMode> {
        self.streaming.as_ref().map(|s| s.mode)
    }

    /// Set the stream mode to ToolUse (message complete, tools pending).
    pub fn set_stream_mode_tool_use(&mut self) {
        if let Some(ref mut streaming) = self.streaming {
            streaming.mode = StreamMode::ToolUse;
        }
    }

    /// Track a new tool use during streaming.
    pub fn add_streaming_tool_use(&mut self, call_id: String, name: String) {
        if let Some(ref mut streaming) = self.streaming {
            streaming.add_tool_use(call_id, name);
        }
    }

    /// Append a tool call delta to the matching streaming tool use.
    pub fn append_tool_call_delta(&mut self, call_id: &str, delta: &str) {
        if let Some(ref mut streaming) = self.streaming {
            streaming.append_tool_call_delta(call_id, delta);
        }
    }

    /// Check if file suggestions are active.
    pub fn has_file_suggestions(&self) -> bool {
        self.file_suggestions.is_some()
    }

    /// Start showing file suggestions.
    pub fn start_file_suggestions(&mut self, query: String, start_pos: i32) {
        self.file_suggestions = Some(FileSuggestionState::new(query, start_pos, true));
    }

    /// Clear file suggestions.
    pub fn clear_file_suggestions(&mut self) {
        self.file_suggestions = None;
    }

    /// Update file suggestions with search results.
    pub fn update_file_suggestions(&mut self, suggestions: Vec<FileSuggestionItem>) {
        if let Some(ref mut state) = self.file_suggestions {
            state.update_suggestions(suggestions);
        }
    }

    /// Toggle display of thinking content.
    pub fn toggle_thinking(&mut self) {
        self.show_thinking = !self.show_thinking;
        tracing::debug!(
            show_thinking = self.show_thinking,
            "Toggled thinking display"
        );
    }

    /// Check if skill suggestions are active.
    pub fn has_skill_suggestions(&self) -> bool {
        self.skill_suggestions.is_some()
    }

    /// Start showing skill suggestions.
    pub fn start_skill_suggestions(&mut self, query: String, start_pos: i32) {
        self.skill_suggestions = Some(SkillSuggestionState::new(query, start_pos, false));
    }

    /// Clear skill suggestions.
    pub fn clear_skill_suggestions(&mut self) {
        self.skill_suggestions = None;
    }

    /// Update skill suggestions with search results.
    pub fn update_skill_suggestions(&mut self, suggestions: Vec<SkillSuggestionItem>) {
        if let Some(ref mut state) = self.skill_suggestions {
            state.update_suggestions(suggestions);
        }
    }

    /// Check if agent suggestions are active.
    pub fn has_agent_suggestions(&self) -> bool {
        self.agent_suggestions.is_some()
    }

    /// Start showing agent suggestions.
    pub fn start_agent_suggestions(&mut self, query: String, start_pos: i32) {
        self.agent_suggestions = Some(AgentSuggestionState::new(query, start_pos, false));
    }

    /// Clear agent suggestions.
    pub fn clear_agent_suggestions(&mut self) {
        self.agent_suggestions = None;
    }

    /// Update agent suggestions with search results.
    pub fn update_agent_suggestions(&mut self, suggestions: Vec<AgentSuggestionItem>) {
        if let Some(ref mut state) = self.agent_suggestions {
            state.update_suggestions(suggestions);
        }
    }

    /// Check if symbol suggestions are active.
    pub fn has_symbol_suggestions(&self) -> bool {
        self.symbol_suggestions.is_some()
    }

    /// Start showing symbol suggestions.
    pub fn start_symbol_suggestions(&mut self, query: String, start_pos: i32) {
        self.symbol_suggestions = Some(SymbolSuggestionState::new(query, start_pos, true));
    }

    /// Clear symbol suggestions.
    pub fn clear_symbol_suggestions(&mut self) {
        self.symbol_suggestions = None;
    }

    /// Update symbol suggestions with search results.
    pub fn update_symbol_suggestions(&mut self, suggestions: Vec<SymbolSuggestionItem>) {
        if let Some(ref mut state) = self.symbol_suggestions {
            state.update_suggestions(suggestions);
        }
    }

    /// Mark that the user has manually scrolled.
    pub fn mark_user_scrolled(&mut self) {
        self.user_scrolled = true;
    }

    /// Reset scroll state for auto-scroll (e.g., when user sends a message).
    pub fn reset_user_scrolled(&mut self) {
        self.user_scrolled = false;
    }

    /// Start the thinking timer.
    pub fn start_thinking(&mut self) {
        if self.thinking_started_at.is_none() {
            self.thinking_started_at = Some(Instant::now());
        }
    }

    /// Stop the thinking timer and record the duration.
    pub fn stop_thinking(&mut self) {
        if let Some(started_at) = self.thinking_started_at.take() {
            self.last_thinking_duration = Some(started_at.elapsed());
        }
    }

    /// Get the current thinking duration (either elapsed or last completed).
    pub fn thinking_duration(&self) -> Option<Duration> {
        if let Some(started_at) = self.thinking_started_at {
            Some(started_at.elapsed())
        } else {
            self.last_thinking_duration
        }
    }

    /// Check if currently thinking.
    pub fn is_thinking(&self) -> bool {
        self.thinking_started_at.is_some()
    }

    /// Clear the last thinking duration (e.g., when starting a new turn).
    pub fn clear_thinking_duration(&mut self) {
        self.last_thinking_duration = None;
    }

    /// Set terminal focus state.
    pub fn set_terminal_focused(&mut self, focused: bool) {
        self.terminal_focused = focused;
        tracing::debug!(focused, "Terminal focus changed");
    }

    // ========== Toast Management ==========

    /// Add a toast notification.
    pub fn add_toast(&mut self, toast: Toast) {
        // Limit to max 5 toasts
        const MAX_TOASTS: usize = 5;
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back(toast);
    }

    /// Add an info toast.
    pub fn toast_info(&mut self, message: impl Into<String>) {
        self.toast_id_counter += 1;
        let toast = Toast::info(format!("toast-{}", self.toast_id_counter), message);
        self.add_toast(toast);
    }

    /// Add a success toast.
    pub fn toast_success(&mut self, message: impl Into<String>) {
        self.toast_id_counter += 1;
        let toast = Toast::success(format!("toast-{}", self.toast_id_counter), message);
        self.add_toast(toast);
    }

    /// Add a warning toast.
    pub fn toast_warning(&mut self, message: impl Into<String>) {
        self.toast_id_counter += 1;
        let toast = Toast::warning(format!("toast-{}", self.toast_id_counter), message);
        self.add_toast(toast);
    }

    /// Add an error toast.
    pub fn toast_error(&mut self, message: impl Into<String>) {
        self.toast_id_counter += 1;
        let toast = Toast::error(format!("toast-{}", self.toast_id_counter), message);
        self.add_toast(toast);
    }

    /// Remove expired toasts.
    pub fn expire_toasts(&mut self) {
        self.toasts.retain(|toast| !toast.is_expired());
    }

    /// Check if there are any active toasts.
    pub fn has_toasts(&self) -> bool {
        !self.toasts.is_empty()
    }

    // ========== Double-Esc Detection ==========

    /// Record an Esc keypress for double-Esc detection.
    pub fn record_esc(&mut self) {
        self.last_esc_time = Some(Instant::now());
    }

    /// Check if the last Esc was within the double-Esc window (800ms).
    pub fn is_double_esc(&self) -> bool {
        self.last_esc_time
            .is_some_and(|t| t.elapsed() < Duration::from_millis(800))
    }

    /// Reset the double-Esc timer.
    pub fn reset_esc_time(&mut self) {
        self.last_esc_time = None;
    }

    // ========== Animation ==========

    /// Increment the animation frame.
    pub fn tick_animation(&mut self) {
        self.animation_frame = (self.animation_frame + 1) % 8;
    }

    /// Get the current animation frame (0-7).
    pub fn animation_frame(&self) -> u8 {
        self.animation_frame
    }
}

/// State for the input field.
#[derive(Debug, Clone, Default)]
pub struct InputState {
    /// The current input text.
    pub text: String,

    /// Cursor position (character index).
    pub cursor: i32,

    /// Selection start (if any).
    pub selection_start: Option<i32>,

    /// History of previous inputs with frecency scores.
    pub history: Vec<HistoryEntry>,

    /// Current history index (for up/down navigation).
    pub history_index: Option<i32>,
}

/// A history entry with frecency scoring.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The command text.
    pub text: String,
    /// Number of times this command was used.
    pub frequency: i32,
    /// Unix timestamp of last use.
    pub last_used: i64,
}

impl HistoryEntry {
    /// Create a new history entry.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            frequency: 1,
            last_used: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
        }
    }

    /// Calculate the frecency score for this entry.
    ///
    /// Higher scores indicate more relevant entries.
    /// Combines frequency with recency decay.
    pub fn frecency_score(&self) -> f64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Time decay: entries older than a day get penalized
        let age_hours = ((now - self.last_used) as f64 / 3600.0).max(0.0);
        let recency_factor = 1.0 / (1.0 + age_hours / 24.0);

        // Frequency boost with diminishing returns
        let frequency_factor = (self.frequency as f64).ln() + 1.0;

        frequency_factor * recency_factor
    }

    /// Update the entry for a new use.
    pub fn mark_used(&mut self) {
        self.frequency += 1;
        self.last_used = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
    }
}

impl InputState {
    /// Insert a character at the cursor position.
    pub fn insert_char(&mut self, c: char) {
        let cursor = self.cursor as usize;
        if cursor >= self.text.len() {
            self.text.push(c);
        } else {
            self.text.insert(cursor, c);
        }
        self.cursor += 1;
    }

    /// Delete the character before the cursor.
    pub fn delete_backward(&mut self) {
        if self.cursor > 0 {
            let cursor = (self.cursor - 1) as usize;
            if cursor < self.text.len() {
                self.text.remove(cursor);
            }
            self.cursor -= 1;
        }
    }

    /// Delete the character at the cursor.
    pub fn delete_forward(&mut self) {
        let cursor = self.cursor as usize;
        if cursor < self.text.len() {
            self.text.remove(cursor);
        }
    }

    /// Move cursor left.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right.
    pub fn move_right(&mut self) {
        if (self.cursor as usize) < self.text.len() {
            self.cursor += 1;
        }
    }

    /// Move cursor to start.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end.
    pub fn move_end(&mut self) {
        self.cursor = self.text.len() as i32;
    }

    /// Move cursor to the start of the previous word.
    pub fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let bytes = self.text.as_bytes();
        let mut pos = (self.cursor - 1) as usize;

        // Skip any whitespace before cursor
        while pos > 0 && bytes[pos].is_ascii_whitespace() {
            pos -= 1;
        }

        // Skip to start of current word
        while pos > 0 && !bytes[pos - 1].is_ascii_whitespace() {
            pos -= 1;
        }

        self.cursor = pos as i32;
    }

    /// Move cursor to the start of the next word.
    pub fn move_word_right(&mut self) {
        let bytes = self.text.as_bytes();
        let len = bytes.len();
        let mut pos = self.cursor as usize;

        if pos >= len {
            return;
        }

        // Skip current word
        while pos < len && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        // Skip whitespace
        while pos < len && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        self.cursor = pos as i32;
    }

    /// Delete the word before the cursor.
    pub fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let original_cursor = self.cursor as usize;
        let bytes = self.text.as_bytes();
        let mut pos = original_cursor;

        // Skip whitespace before cursor
        while pos > 0 && bytes[pos - 1].is_ascii_whitespace() {
            pos -= 1;
        }

        // Skip to start of word
        while pos > 0 && !bytes[pos - 1].is_ascii_whitespace() {
            pos -= 1;
        }

        // Remove the text between pos and original cursor
        let text = &self.text;
        self.text = format!("{}{}", &text[..pos], &text[original_cursor..]);
        self.cursor = pos as i32;
    }

    /// Delete the word after the cursor.
    pub fn delete_word_forward(&mut self) {
        let bytes = self.text.as_bytes();
        let len = bytes.len();
        let start_pos = self.cursor as usize;

        if start_pos >= len {
            return;
        }

        let mut pos = start_pos;

        // Skip current word
        while pos < len && !bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        // Skip whitespace after word
        while pos < len && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        // Remove the text between start_pos and pos
        let text = &self.text;
        self.text = format!("{}{}", &text[..start_pos], &text[pos..]);
        // Cursor stays at same position
    }

    /// Insert a newline.
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Clear the input and return the text.
    pub fn take(&mut self) -> String {
        let text = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.selection_start = None;
        text
    }

    /// Check if the input is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Get the current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Set the text (e.g., from history or paste).
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len() as i32;
    }

    /// Add text to history with frecency tracking.
    ///
    /// If the text already exists in history, updates its frecency.
    /// Otherwise, adds a new entry.
    pub fn add_to_history(&mut self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }

        // Check if entry already exists
        if let Some(entry) = self.history.iter_mut().find(|e| e.text == text) {
            entry.mark_used();
        } else {
            self.history.push(HistoryEntry::new(text));
        }

        // Sort by frecency (highest first)
        self.history.sort_by(|a, b| {
            b.frecency_score()
                .partial_cmp(&a.frecency_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit history size
        const MAX_HISTORY: usize = 100;
        if self.history.len() > MAX_HISTORY {
            self.history.truncate(MAX_HISTORY);
        }
    }

    /// Get a history entry by index.
    pub fn history_text(&self, index: usize) -> Option<&str> {
        self.history.get(index).map(|e| e.text.as_str())
    }

    /// Get the number of history entries.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Detect an @mention at or before the cursor and return the query.
    ///
    /// Returns `Some((start_pos, query))` if there's an @mention being typed,
    /// where `start_pos` is the position of the @ character and `query` is
    /// the text after @.
    ///
    /// An @mention is detected when:
    /// - There's an @ character before the cursor
    /// - The @ is either at the start or preceded by whitespace
    /// - There's no space between @ and the cursor (unless in quoted mode)
    ///
    /// Supports quoted paths: `@"path with spaces"` — the opening `"`
    /// after `@` starts a quoted context where spaces are allowed.
    pub fn current_at_token(&self) -> Option<(i32, String)> {
        let text = &self.text;
        let cursor = self.cursor as usize;

        if cursor == 0 || text.is_empty() {
            return None;
        }

        let before_cursor = &text[..cursor.min(text.len())];

        // Check for quoted mode: @"... (look backwards for @")
        // Find the last @ before cursor
        for (i, c) in before_cursor.char_indices().rev() {
            if c == '@' {
                // Check if @ is at start or preceded by whitespace
                let is_valid_start = i == 0
                    || before_cursor[..i]
                        .chars()
                        .last()
                        .is_some_and(char::is_whitespace);
                if !is_valid_start {
                    return None;
                }

                let after_at = &before_cursor[i + 1..];

                // Check for quoted mode: @"...
                if let Some(rest) = after_at.strip_prefix('"') {
                    // In quoted mode — extract text inside quotes (closing quote optional)
                    let query = if let Some(close) = rest.find('"') {
                        // Closing quote found — mention is complete, no active token
                        // (unless cursor is right on the closing quote)
                        if i + 2 + close < cursor {
                            return None;
                        }
                        rest[..close].to_string()
                    } else {
                        rest.to_string()
                    };
                    return Some((i as i32, query));
                }

                // Normal (unquoted) mode — no whitespace allowed between @ and cursor
                if after_at.contains([' ', '\n', '\t']) {
                    return None;
                }
                return Some((i as i32, after_at.to_string()));
            }

            // In unquoted mode, whitespace means no active mention
            // But in quoted mode (@"path with spaces"), spaces are allowed
            if c == ' ' || c == '\n' || c == '\t' {
                let prefix = &before_cursor[..i];
                if let Some(at_quote_pos) = prefix.rfind("@\"") {
                    let valid_start = at_quote_pos == 0
                        || prefix[..at_quote_pos]
                            .chars()
                            .last()
                            .is_some_and(char::is_whitespace);
                    let after_open = &prefix[at_quote_pos + 2..];
                    if valid_start && !after_open.contains('"') {
                        continue;
                    }
                }
                break;
            }
        }

        None
    }

    /// Detect a /command token at cursor position.
    ///
    /// Returns `Some((start_pos, query))` if there's a slash command being typed,
    /// where `start_pos` is the position of the / character and `query` is
    /// the text after /.
    ///
    /// A slash command is detected when:
    /// - There's a / character before the cursor
    /// - The / is at the start of input or preceded by whitespace
    /// - There's no space between / and the cursor
    pub fn current_slash_token(&self) -> Option<(i32, String)> {
        let text = &self.text;
        let cursor = self.cursor as usize;

        if cursor == 0 || text.is_empty() {
            return None;
        }

        // Look backwards from cursor for /
        let before_cursor = &text[..cursor.min(text.len())];

        // Find the last / before cursor that isn't followed by a space
        let mut slash_pos = None;
        for (i, c) in before_cursor.char_indices().rev() {
            if c == ' ' || c == '\n' || c == '\t' {
                // Hit whitespace without finding /, no active command
                break;
            }
            if c == '/' {
                // Check if / is at start or preceded by whitespace
                if i == 0 {
                    slash_pos = Some(i);
                } else {
                    let prev_char = before_cursor[..i].chars().last();
                    if prev_char.is_some_and(char::is_whitespace) {
                        slash_pos = Some(i);
                    }
                }
                break;
            }
        }

        slash_pos.map(|pos| {
            let query = before_cursor[pos + 1..].to_string();
            (pos as i32, query)
        })
    }

    /// Insert a selected skill name, replacing the current /query.
    ///
    /// The `start_pos` is the position of the / character, and `name` is
    /// the skill name to insert (without the /).
    pub fn insert_selected_skill(&mut self, start_pos: i32, name: &str) {
        let start = start_pos as usize;
        let cursor = self.cursor as usize;

        if start >= self.text.len() || cursor > self.text.len() {
            return;
        }

        // Build new text: before / + /name + space + after cursor
        let before = &self.text[..start];
        let after = &self.text[cursor..];
        let new_text = format!("{before}/{name} {after}");

        // Calculate new cursor position: after the inserted skill name and space
        let new_cursor = start + 1 + name.len() + 1;

        self.text = new_text;
        self.cursor = new_cursor as i32;
    }

    /// Insert a selected agent type, replacing the current @query.
    ///
    /// The `start_pos` is the position of the @ character, and `agent_type` is
    /// the agent type to insert (e.g., "explore" → `@agent-explore `).
    pub fn insert_selected_agent(&mut self, start_pos: i32, agent_type: &str) {
        let start = start_pos as usize;
        let cursor = self.cursor as usize;

        if start >= self.text.len() || cursor > self.text.len() {
            return;
        }

        // Build new text: before @ + @agent-type + space + after cursor
        let before = &self.text[..start];
        let after = &self.text[cursor..];
        let mention = format!("agent-{agent_type}");
        let new_text = format!("{before}@{mention} {after}");

        // Calculate new cursor position: after the inserted agent mention and space
        let new_cursor = start + 1 + mention.len() + 1;

        self.text = new_text;
        self.cursor = new_cursor as i32;
    }

    /// Insert a selected symbol, replacing the current @#query with @filepath:line.
    ///
    /// The `start_pos` is the position of the @ character, and `file_path` / `line`
    /// identify the symbol's location.
    pub fn insert_selected_symbol(&mut self, start_pos: i32, file_path: &str, line: i32) {
        let start = start_pos as usize;
        let cursor = self.cursor as usize;

        if start >= self.text.len() || cursor > self.text.len() {
            return;
        }

        // Build new text: before @ + @filepath:line + space + after cursor
        let before = &self.text[..start];
        let after = &self.text[cursor..];
        let mention = format!("{file_path}:{line}");
        let new_text = format!("{before}@{mention} {after}");

        // Calculate new cursor position: after the inserted mention and space
        let new_cursor = start + 1 + mention.len() + 1;

        self.text = new_text;
        self.cursor = new_cursor as i32;
    }

    /// Insert a selected file path, replacing the current @query.
    ///
    /// The `start_pos` is the position of the @ character, and `path` is
    /// the path to insert (without the @).
    /// If the path contains spaces, it is wrapped in quotes: `@"path with spaces"`.
    pub fn insert_selected_path(&mut self, start_pos: i32, path: &str) {
        let start = start_pos as usize;
        let cursor = self.cursor as usize;

        if start >= self.text.len() || cursor > self.text.len() {
            return;
        }

        let before = &self.text[..start];
        let after = &self.text[cursor..];

        if path.contains(' ') {
            // Quoted path: @"path with spaces"
            let new_text = format!("{before}@\"{path}\" {after}");
            let new_cursor = start + 2 + path.len() + 2; // @" + path + " + space
            self.text = new_text;
            self.cursor = new_cursor as i32;
        } else {
            // Normal path: @path
            let new_text = format!("{before}@{path} {after}");
            let new_cursor = start + 1 + path.len() + 1;
            self.text = new_text;
            self.cursor = new_cursor as i32;
        }
    }
}

/// The current focus target in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusTarget {
    /// Input field is focused.
    #[default]
    Input,
    /// Chat history is focused (for scrolling).
    Chat,
    /// Tool panel is focused.
    ToolPanel,
}

/// An active overlay (modal dialog).
#[derive(Debug, Clone)]
pub enum Overlay {
    /// Permission approval prompt.
    Permission(PermissionOverlay),
    /// Plan mode exit approval with 5 options.
    PlanExitApproval(PlanExitOverlay),
    /// Question overlay for AskUserQuestion tool.
    Question(QuestionOverlay),
    /// Model picker.
    ModelPicker(ModelPickerOverlay),
    /// Output style picker.
    OutputStylePicker(OutputStylePickerOverlay),
    /// Command palette.
    CommandPalette(CommandPaletteOverlay),
    /// Session browser.
    SessionBrowser(SessionBrowserOverlay),
    /// Plugin manager (4-tab interface).
    PluginManager(PluginManagerOverlay),
    /// Rewind selector (checkpoint browser + mode picker).
    RewindSelector(RewindSelectorOverlay),
    /// Help screen.
    Help,
    /// Error message.
    Error(String),
}

impl Overlay {
    /// Get the priority of this overlay (lower = higher priority).
    ///
    /// Matches Claude Code's dialog priority dispatcher:
    /// - Permission/PlanExit: highest (security-critical, blocks execution)
    /// - Question: high (tool needs user input to continue)
    /// - Error: medium (user must acknowledge)
    /// - User-triggered overlays (model picker, etc.): lowest
    pub fn priority(&self) -> i32 {
        match self {
            Overlay::Permission(_) | Overlay::PlanExitApproval(_) => 0,
            Overlay::Question(_) => 1,
            Overlay::Error(_) => 2,
            Overlay::RewindSelector(_) => 3,
            Overlay::PluginManager(_) => 4,
            Overlay::ModelPicker(_) => 5,
            Overlay::OutputStylePicker(_) => 5,
            Overlay::CommandPalette(_) => 5,
            Overlay::SessionBrowser(_) => 5,
            Overlay::Help => 6,
        }
    }

    /// Whether this overlay type should be queued when another is active.
    ///
    /// Agent-driven overlays (permissions, questions) are queueable because
    /// multiple can arrive concurrently. User-triggered overlays replace
    /// the current one since the user explicitly requested them.
    pub fn is_agent_driven(&self) -> bool {
        matches!(
            self,
            Overlay::Permission(_)
                | Overlay::PlanExitApproval(_)
                | Overlay::Question(_)
                | Overlay::Error(_)
        )
    }
}

/// Permission approval overlay state.
#[derive(Debug, Clone)]
pub struct PermissionOverlay {
    /// The approval request.
    pub request: ApprovalRequest,
    /// Selected option index (0 = approve, 1 = deny, 2 = approve all).
    pub selected: i32,
}

impl PermissionOverlay {
    /// Create a new permission overlay.
    pub fn new(request: ApprovalRequest) -> Self {
        Self {
            request,
            selected: 0,
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        if self.selected < 2 {
            self.selected += 1;
        }
    }
}

/// Plan mode exit approval overlay.
///
/// Provides 5 options matching Claude Code:
/// 0 = Clear context + accept edits (Shift+Tab)
/// 1 = Clear context + bypass
/// 2 = Keep context + elevate to accept-edits
/// 3 = Keep context + manual approve (restore pre-plan mode)
/// 4 = Keep planning (deny) — supports feedback text input
#[derive(Debug, Clone)]
pub struct PlanExitOverlay {
    /// The approval request from ExitPlanMode.
    pub request: ApprovalRequest,
    /// Selected option index (0-4).
    pub selected: i32,
    /// Plan preview text (first ~2000 chars of plan).
    pub plan_preview: String,
    /// Whether the feedback text input is active (option 4 selected + Enter pressed).
    pub feedback_active: bool,
    /// Feedback text for "keep planning" option.
    pub feedback_text: String,
}

impl PlanExitOverlay {
    /// Create a new plan exit overlay.
    pub fn new(request: ApprovalRequest) -> Self {
        // Extract plan preview from the request description
        let plan_preview = request
            .description
            .strip_prefix("Exit plan mode?\n\n## Implementation Plan\n\n")
            .unwrap_or(&request.description)
            .to_string();
        Self {
            request,
            selected: 0,
            plan_preview,
            feedback_active: false,
            feedback_text: String::new(),
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        if self.selected < 4 {
            self.selected += 1;
        }
    }

    /// Get the `PlanExitOption` for the current selection.
    pub fn selected_option(&self) -> cocode_protocol::PlanExitOption {
        match self.selected {
            0 => cocode_protocol::PlanExitOption::ClearAndAcceptEdits,
            1 => cocode_protocol::PlanExitOption::ClearAndBypass,
            2 => cocode_protocol::PlanExitOption::KeepAndElevate,
            3 => cocode_protocol::PlanExitOption::KeepAndDefault,
            _ => cocode_protocol::PlanExitOption::KeepPlanning,
        }
    }
}

/// Question overlay for AskUserQuestion tool.
///
/// Displays 1-4 questions with selectable options. Each question can be
/// single-select or multi-select. The user navigates with Up/Down and
/// confirms with Enter.
#[derive(Debug, Clone)]
pub struct QuestionOverlay {
    /// Unique request ID (for correlating the response).
    pub request_id: String,
    /// The questions to display.
    pub questions: Vec<QuestionItem>,
    /// Index of the currently focused question.
    pub current_question: i32,
    /// Whether we're in the "Other" text input mode for the current question.
    pub other_input_active: bool,
    /// Text buffer for "Other" input.
    pub other_text: String,
}

/// A single question in the question overlay.
#[derive(Debug, Clone)]
pub struct QuestionItem {
    /// The full question text.
    pub question: String,
    /// Short header label.
    pub header: String,
    /// Available options.
    pub options: Vec<QuestionOption>,
    /// Whether multiple options can be selected.
    pub multi_select: bool,
    /// Currently highlighted option index (includes "Other" at the end).
    pub selected: i32,
    /// For multi-select: which options are checked.
    pub checked: Vec<bool>,
    /// User's confirmed answer (set when moving to next question).
    pub answer: Option<String>,
}

/// A single option for a question.
#[derive(Debug, Clone)]
pub struct QuestionOption {
    /// Display label.
    pub label: String,
    /// Description.
    pub description: String,
}

impl QuestionOverlay {
    /// Create from JSON questions array.
    pub fn new(request_id: String, questions: &serde_json::Value) -> Self {
        let items = questions
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|q| {
                        let options: Vec<QuestionOption> = q["options"]
                            .as_array()
                            .map(|opts| {
                                opts.iter()
                                    .map(|o| QuestionOption {
                                        label: o["label"].as_str().unwrap_or("").to_string(),
                                        description: o["description"]
                                            .as_str()
                                            .unwrap_or("")
                                            .to_string(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let option_count = options.len();
                        QuestionItem {
                            question: q["question"].as_str().unwrap_or("").to_string(),
                            header: q["header"].as_str().unwrap_or("").to_string(),
                            multi_select: q["multiSelect"].as_bool().unwrap_or(false),
                            options,
                            selected: 0,
                            checked: vec![false; option_count],
                            answer: None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        Self {
            request_id,
            questions: items,
            current_question: 0,
            other_input_active: false,
            other_text: String::new(),
        }
    }

    /// Get the current question.
    pub fn current(&self) -> Option<&QuestionItem> {
        self.questions.get(self.current_question as usize)
    }

    /// Get the current question mutably.
    pub fn current_mut(&mut self) -> Option<&mut QuestionItem> {
        self.questions.get_mut(self.current_question as usize)
    }

    /// Move selection up within the current question.
    pub fn move_up(&mut self) {
        if let Some(q) = self.current_mut() {
            if q.selected > 0 {
                q.selected -= 1;
            }
            // If we moved off "Other", deactivate text input
            self.other_input_active = false;
        }
    }

    /// Move selection down within the current question.
    pub fn move_down(&mut self) {
        if let Some(q) = self.current_mut() {
            // +1 for "Other" option at the end
            let max = q.options.len() as i32;
            if q.selected < max {
                q.selected += 1;
            }
        }
    }

    /// Toggle the currently selected option (for multi-select).
    pub fn toggle_selected(&mut self) {
        if let Some(q) = self.current_mut() {
            let idx = q.selected as usize;
            if idx < q.checked.len() {
                q.checked[idx] = !q.checked[idx];
            }
        }
    }

    /// Check if the current selection is "Other".
    pub fn is_other_selected(&self) -> bool {
        self.current()
            .is_some_and(|q| q.selected as usize == q.options.len())
    }

    /// Confirm the current question and advance to the next.
    ///
    /// Returns `true` when all questions are answered (ready to submit).
    pub fn confirm_current(&mut self) -> bool {
        if self.other_input_active && self.is_other_selected() {
            // "Other" text confirmed
            let text = self.other_text.clone();
            if let Some(q) = self.current_mut() {
                q.answer = Some(text);
            }
            self.other_input_active = false;
            self.other_text.clear();
        } else if self.is_other_selected() {
            // Activate "Other" text input
            self.other_input_active = true;
            self.other_text.clear();
            return false;
        } else if let Some(q) = self.current_mut() {
            if q.multi_select {
                // For multi-select, collect all checked labels
                let selected: Vec<String> = q
                    .options
                    .iter()
                    .zip(q.checked.iter())
                    .filter(|&(_, &checked)| checked)
                    .map(|(opt, _)| opt.label.clone())
                    .collect();
                q.answer = Some(selected.join(", "));
            } else {
                // For single-select, use the highlighted option
                let idx = q.selected as usize;
                if let Some(opt) = q.options.get(idx) {
                    q.answer = Some(opt.label.clone());
                }
            }
        }

        // Advance to next unanswered question
        self.current_question += 1;
        self.current_question as usize >= self.questions.len()
    }

    /// Collect all answers as a JSON object keyed by question text.
    pub fn collect_answers(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for q in &self.questions {
            let answer = q.answer.clone().unwrap_or_else(|| "No answer".to_string());
            map.insert(q.question.clone(), serde_json::Value::String(answer));
        }
        serde_json::Value::Object(map)
    }
}

/// Model picker overlay state.
#[derive(Debug, Clone)]
pub struct ModelPickerOverlay {
    /// Available model selections.
    pub items: Vec<RoleSelection>,
    /// Currently selected index.
    pub selected: i32,
    /// Search filter.
    pub filter: String,
}

impl ModelPickerOverlay {
    /// Create a new model picker.
    pub fn new(items: Vec<RoleSelection>) -> Self {
        Self {
            items,
            selected: 0,
            filter: String::new(),
        }
    }

    /// Get filtered items.
    pub fn filtered_items(&self) -> Vec<&RoleSelection> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let filter = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|s| s.model.to_string().to_lowercase().contains(&filter))
                .collect()
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        let max = self.filtered_items().len() as i32 - 1;
        if self.selected < max {
            self.selected += 1;
        }
    }
}

/// Output style picker overlay state.
#[derive(Debug, Clone)]
pub struct OutputStylePickerOverlay {
    /// Available style entries: (name, source_label, description).
    pub items: Vec<OutputStylePickerItem>,
    /// Currently selected index.
    pub selected: i32,
    /// Search filter.
    pub filter: String,
}

/// A single item in the output style picker.
#[derive(Debug, Clone)]
pub struct OutputStylePickerItem {
    /// Style name.
    pub name: String,
    /// Source label (e.g. "built-in", "custom", "project", "plugin").
    pub source: String,
    /// Optional description.
    pub description: Option<String>,
}

impl OutputStylePickerOverlay {
    /// Create a new output style picker.
    pub fn new(items: Vec<OutputStylePickerItem>) -> Self {
        Self {
            items,
            selected: 0,
            filter: String::new(),
        }
    }

    /// Get filtered items.
    pub fn filtered_items(&self) -> Vec<&OutputStylePickerItem> {
        if self.filter.is_empty() {
            self.items.iter().collect()
        } else {
            let filter = self.filter.to_lowercase();
            self.items
                .iter()
                .filter(|s| s.name.to_lowercase().contains(&filter))
                .collect()
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        let max = self.filtered_items().len() as i32 - 1;
        if self.selected < max {
            self.selected += 1;
        }
    }
}

/// Command palette overlay state.
#[derive(Debug, Clone)]
pub struct CommandPaletteOverlay {
    /// Search query.
    pub query: String,
    /// All available commands.
    pub commands: Vec<CommandItem>,
    /// Indices of filtered commands.
    pub filtered: Vec<i32>,
    /// Currently selected index in filtered list.
    pub selected: i32,
}

impl CommandPaletteOverlay {
    /// Create a new command palette.
    pub fn new(commands: Vec<CommandItem>) -> Self {
        let filtered: Vec<i32> = (0..commands.len() as i32).collect();
        Self {
            query: String::new(),
            commands,
            filtered,
            selected: 0,
        }
    }

    /// Update the filter based on query.
    pub fn update_filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.commands.len() as i32).collect();
        } else {
            let query = self.query.to_lowercase();
            self.filtered = self
                .commands
                .iter()
                .enumerate()
                .filter(|(_, cmd)| {
                    cmd.name.to_lowercase().contains(&query)
                        || cmd.description.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i as i32)
                .collect();
        }
        // Reset selection if out of bounds
        if self.selected >= self.filtered.len() as i32 {
            self.selected = 0;
        }
    }

    /// Get filtered commands.
    pub fn filtered_commands(&self) -> Vec<&CommandItem> {
        self.filtered
            .iter()
            .filter_map(|&i| self.commands.get(i as usize))
            .collect()
    }

    /// Get the currently selected command.
    pub fn selected_command(&self) -> Option<&CommandItem> {
        self.filtered
            .get(self.selected as usize)
            .and_then(|&i| self.commands.get(i as usize))
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        let max = (self.filtered.len() as i32).saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Add a character to the query.
    pub fn insert_char(&mut self, c: char) {
        self.query.push(c);
        self.update_filter();
    }

    /// Delete a character from the query.
    pub fn delete_char(&mut self) {
        self.query.pop();
        self.update_filter();
    }
}

/// A command item in the command palette.
#[derive(Debug, Clone)]
pub struct CommandItem {
    /// Command name/label.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Keyboard shortcut (if any).
    pub shortcut: Option<String>,
    /// The action to execute.
    pub action: CommandAction,
}

/// Action to execute when a command is selected.
#[derive(Debug, Clone)]
pub enum CommandAction {
    /// Toggle plan mode.
    TogglePlanMode,
    /// Cycle thinking level.
    CycleThinkingLevel,
    /// Show model picker.
    ShowModelPicker,
    /// Show help.
    ShowHelp,
    /// Show session browser.
    ShowSessionBrowser,
    /// Show plugin manager.
    ShowPluginManager,
    /// Clear screen.
    ClearScreen,
    /// Interrupt.
    Interrupt,
    /// Quit.
    Quit,
}

/// Session browser overlay state.
#[derive(Debug, Clone)]
pub struct SessionBrowserOverlay {
    /// Available sessions.
    pub sessions: Vec<SessionSummary>,
    /// Currently selected index.
    pub selected: i32,
    /// Search filter.
    pub filter: String,
}

impl SessionBrowserOverlay {
    /// Create a new session browser.
    pub fn new(sessions: Vec<SessionSummary>) -> Self {
        Self {
            sessions,
            selected: 0,
            filter: String::new(),
        }
    }

    /// Get filtered sessions.
    pub fn filtered_sessions(&self) -> Vec<&SessionSummary> {
        if self.filter.is_empty() {
            self.sessions.iter().collect()
        } else {
            let filter = self.filter.to_lowercase();
            self.sessions
                .iter()
                .filter(|s| {
                    s.title.to_lowercase().contains(&filter)
                        || s.id.to_lowercase().contains(&filter)
                })
                .collect()
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        let max = (self.filtered_sessions().len() as i32).saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Get the currently selected session.
    pub fn selected_session(&self) -> Option<&SessionSummary> {
        let filtered = self.filtered_sessions();
        filtered.get(self.selected as usize).copied()
    }

    /// Add a character to the filter.
    pub fn insert_char(&mut self, c: char) {
        self.filter.push(c);
    }

    /// Delete a character from the filter.
    pub fn delete_char(&mut self) {
        self.filter.pop();
    }
}

/// Summary of a saved session.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    /// Session ID.
    pub id: String,
    /// Session title/description.
    pub title: String,
    /// Creation timestamp (Unix seconds).
    pub created_at: i64,
    /// Last modified timestamp (Unix seconds).
    pub updated_at: i64,
    /// Number of messages in the session.
    pub message_count: i32,
}

/// Active tab in the plugin manager overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginManagerTab {
    /// Discover plugins from marketplaces.
    #[default]
    Discover,
    /// View/manage installed plugins.
    Installed,
    /// Manage marketplace sources.
    Marketplaces,
    /// View plugin loading errors.
    Errors,
}

impl PluginManagerTab {
    /// Advance to the next tab.
    pub fn next(self) -> Self {
        match self {
            Self::Discover => Self::Installed,
            Self::Installed => Self::Marketplaces,
            Self::Marketplaces => Self::Errors,
            Self::Errors => Self::Discover,
        }
    }

    /// Go to the previous tab.
    pub fn prev(self) -> Self {
        match self {
            Self::Discover => Self::Errors,
            Self::Installed => Self::Discover,
            Self::Marketplaces => Self::Installed,
            Self::Errors => Self::Marketplaces,
        }
    }
}

/// Summary of a plugin for display in the plugin manager.
#[derive(Debug, Clone)]
pub struct PluginSummary {
    /// Plugin name (kebab-case identifier).
    pub name: String,
    /// Short description.
    pub description: String,
    /// Version string.
    pub version: String,
    /// Whether the plugin is currently enabled.
    pub enabled: bool,
    /// Installation scope (user/project/managed).
    pub scope: String,
    /// Number of skills contributed.
    pub skills_count: i32,
    /// Number of hooks contributed.
    pub hooks_count: i32,
    /// Number of agents contributed.
    pub agents_count: i32,
}

/// Summary of a marketplace source.
#[derive(Debug, Clone)]
pub struct MarketplaceSummary {
    /// Marketplace name.
    pub name: String,
    /// Source type (GitHub, Git, URL, etc.).
    pub source_type: String,
    /// Source URL or path.
    pub source: String,
    /// Whether auto-update is enabled.
    pub auto_update: bool,
    /// Number of plugins in this marketplace.
    pub plugin_count: i32,
}

/// A plugin loading error entry.
#[derive(Debug, Clone)]
pub struct PluginErrorEntry {
    /// Plugin name or path that failed.
    pub source: String,
    /// Error message.
    pub error: String,
}

/// Plugin manager overlay state.
#[derive(Debug, Clone)]
pub struct PluginManagerOverlay {
    /// Current active tab.
    pub tab: PluginManagerTab,
    /// Selected index in the current tab's list.
    pub selected: i32,
    /// Search/filter text.
    pub filter: String,
    /// Discovered plugins from marketplaces.
    pub discover_items: Vec<PluginSummary>,
    /// Installed plugins.
    pub installed_items: Vec<PluginSummary>,
    /// Known marketplaces.
    pub marketplace_items: Vec<MarketplaceSummary>,
    /// Plugin loading errors.
    pub error_items: Vec<PluginErrorEntry>,
}

impl PluginManagerOverlay {
    /// Create a new plugin manager overlay.
    pub fn new(
        installed: Vec<PluginSummary>,
        marketplaces: Vec<MarketplaceSummary>,
        errors: Vec<PluginErrorEntry>,
    ) -> Self {
        Self {
            tab: PluginManagerTab::default(),
            selected: 0,
            filter: String::new(),
            discover_items: Vec::new(),
            installed_items: installed,
            marketplace_items: marketplaces,
            error_items: errors,
        }
    }

    /// Get the count of items in the current tab.
    fn current_item_count(&self) -> i32 {
        match self.tab {
            PluginManagerTab::Discover => self.filtered_discover().len() as i32,
            PluginManagerTab::Installed => self.filtered_installed().len() as i32,
            PluginManagerTab::Marketplaces => self.marketplace_items.len() as i32,
            PluginManagerTab::Errors => self.error_items.len() as i32,
        }
    }

    /// Get filtered discover items.
    pub fn filtered_discover(&self) -> Vec<&PluginSummary> {
        if self.filter.is_empty() {
            self.discover_items.iter().collect()
        } else {
            let filter = self.filter.to_lowercase();
            self.discover_items
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&filter)
                        || p.description.to_lowercase().contains(&filter)
                })
                .collect()
        }
    }

    /// Get filtered installed items.
    pub fn filtered_installed(&self) -> Vec<&PluginSummary> {
        if self.filter.is_empty() {
            self.installed_items.iter().collect()
        } else {
            let filter = self.filter.to_lowercase();
            self.installed_items
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&filter)
                        || p.description.to_lowercase().contains(&filter)
                })
                .collect()
        }
    }

    /// Switch to the next tab, resetting selection.
    pub fn next_tab(&mut self) {
        self.tab = self.tab.next();
        self.selected = 0;
        self.filter.clear();
    }

    /// Switch to the previous tab, resetting selection.
    pub fn prev_tab(&mut self) {
        self.tab = self.tab.prev();
        self.selected = 0;
        self.filter.clear();
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        let max = self.current_item_count().saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Add a character to the filter.
    pub fn insert_char(&mut self, c: char) {
        self.filter.push(c);
        // Reset selection when filter changes
        self.selected = 0;
    }

    /// Delete a character from the filter.
    pub fn delete_char(&mut self) {
        self.filter.pop();
        self.selected = 0;
    }
}

/// Current phase of streaming from the LLM.
///
/// Tracks what the model is currently outputting, used for spinner display
/// and status text. Matches Claude Code's 5-mode streaming state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamMode {
    /// Request is being sent to the API.
    #[default]
    Requesting,
    /// Text content is being streamed.
    Responding,
    /// Extended thinking content is being streamed.
    Thinking,
    /// Tool call input JSON is being streamed.
    ToolInput,
    /// Message complete, tool execution pending.
    ToolUse,
}

/// A tool use being accumulated during streaming.
///
/// Tracks partial JSON input as it arrives via `ToolCallDelta` events,
/// allowing the UI to display in-progress tool call details.
#[derive(Debug, Clone)]
pub struct StreamingToolUse {
    /// Tool call identifier.
    pub call_id: String,
    /// Tool name.
    pub name: String,
    /// Accumulated partial JSON input (not yet parseable).
    pub accumulated_input: String,
}

/// State for streaming content.
#[derive(Debug, Clone)]
pub struct StreamingState {
    /// Turn identifier.
    pub turn_id: String,
    /// Content being streamed.
    pub content: String,
    /// Thinking content being streamed.
    pub thinking: String,
    /// Current streaming mode.
    pub mode: StreamMode,
    /// Tool uses being accumulated during streaming.
    pub tool_uses: Vec<StreamingToolUse>,
}

impl StreamingState {
    /// Create new streaming state.
    pub fn new(turn_id: String) -> Self {
        Self {
            turn_id,
            content: String::new(),
            thinking: String::new(),
            mode: StreamMode::Requesting,
            tool_uses: Vec::new(),
        }
    }

    /// Get estimated thinking token count (rough estimate: words * 1.3).
    pub fn thinking_tokens(&self) -> i32 {
        let word_count = self.thinking.split_whitespace().count();
        (word_count as f64 * 1.3) as i32
    }

    /// Append a tool call delta to the matching streaming tool use.
    pub fn append_tool_call_delta(&mut self, call_id: &str, delta: &str) {
        if let Some(tool) = self.tool_uses.iter_mut().find(|t| t.call_id == call_id) {
            tool.accumulated_input.push_str(delta);
        } else {
            tracing::debug!(call_id, "Tool call delta for unknown call_id");
        }
    }

    /// Start tracking a new tool use.
    pub fn add_tool_use(&mut self, call_id: String, name: String) {
        self.tool_uses.push(StreamingToolUse {
            call_id,
            name,
            accumulated_input: String::new(),
        });
        self.mode = StreamMode::ToolInput;
    }
}

/// Tracks query timing with permission-pause exclusion.
///
/// Models Claude Code's timing refs pattern: query start time, cumulative
/// permission pause duration, and slow-query threshold detection.
///
/// When the user is viewing a permission dialog, that time is excluded from
/// the query duration so the "slow query" notification reflects actual LLM
/// processing time, not time spent waiting for human approval.
#[derive(Debug, Clone, Default)]
pub struct QueryTiming {
    /// When the current query started (set on `TurnStarted`).
    start: Option<Instant>,
    /// When the current permission dialog opened (for pause tracking).
    pause_start: Option<Instant>,
    /// Cumulative duration spent in permission dialogs during the current query.
    total_paused: Duration,
}

impl QueryTiming {
    /// Slow query threshold — matches Claude Code's 30-second notification.
    const SLOW_QUERY_THRESHOLD: Duration = Duration::from_secs(30);

    /// Start tracking a new query.
    pub fn start(&mut self) {
        self.start = Some(Instant::now());
        self.pause_start = None;
        self.total_paused = Duration::ZERO;
    }

    /// Record that a permission dialog has opened (start pausing the clock).
    pub fn on_permission_dialog_open(&mut self) {
        if self.pause_start.is_none() {
            self.pause_start = Some(Instant::now());
        }
    }

    /// Record that a permission dialog has closed (resume the clock).
    pub fn on_permission_dialog_close(&mut self) {
        if let Some(pause_start) = self.pause_start.take() {
            self.total_paused += pause_start.elapsed();
        }
    }

    /// Get the actual query duration (excluding permission pause time).
    pub fn actual_duration(&self) -> Option<Duration> {
        self.start
            .map(|s| s.elapsed().saturating_sub(self.total_paused))
    }

    /// Check if the query exceeds the slow-query threshold.
    pub fn is_slow_query(&self) -> bool {
        self.actual_duration()
            .is_some_and(|d| d > Self::SLOW_QUERY_THRESHOLD)
    }

    /// Stop tracking (query completed or aborted).
    pub fn stop(&mut self) {
        // Close any open pause before stopping
        if let Some(pause_start) = self.pause_start.take() {
            self.total_paused += pause_start.elapsed();
        }
        self.start = None;
    }

    /// Whether a query is being tracked.
    pub fn is_active(&self) -> bool {
        self.start.is_some()
    }
}

/// Generic suggestion state for all autocomplete types.
///
/// Provides shared navigation logic (move_up, move_down, selected_suggestion,
/// update_suggestions) so that each autocomplete system only needs to define
/// its item type.
#[derive(Debug, Clone)]
pub struct SuggestionState<T> {
    /// The query text (without trigger prefix like @, /, @#).
    pub query: String,
    /// Start position of the trigger in the input text.
    pub start_pos: i32,
    /// Current suggestions.
    pub suggestions: Vec<T>,
    /// Currently selected index in the dropdown.
    pub selected: i32,
    /// Whether a search is currently in progress.
    pub loading: bool,
}

impl<T> SuggestionState<T> {
    /// Create a new suggestion state.
    pub fn new(query: String, start_pos: i32, loading: bool) -> Self {
        Self {
            query,
            start_pos,
            suggestions: Vec::new(),
            selected: 0,
            loading,
        }
    }

    /// Move selection up.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down.
    pub fn move_down(&mut self) {
        let max = (self.suggestions.len() as i32).saturating_sub(1);
        if self.selected < max {
            self.selected += 1;
        }
    }

    /// Get the currently selected suggestion.
    pub fn selected_suggestion(&self) -> Option<&T> {
        self.suggestions.get(self.selected as usize)
    }

    /// Update suggestions from search results.
    pub fn update_suggestions(&mut self, suggestions: Vec<T>) {
        self.suggestions = suggestions;
        self.loading = false;
        // Reset selection if out of bounds
        if self.selected >= self.suggestions.len() as i32 {
            self.selected = 0;
        }
    }
}

/// File suggestion state (autocomplete for `@path` mentions).
pub type FileSuggestionState = SuggestionState<FileSuggestionItem>;
/// Skill suggestion state (autocomplete for `/command` slash commands).
pub type SkillSuggestionState = SuggestionState<SkillSuggestionItem>;
/// Agent suggestion state (autocomplete for `@agent-*` mentions).
pub type AgentSuggestionState = SuggestionState<AgentSuggestionItem>;
/// Symbol suggestion state (autocomplete for `@#symbol` mentions).
pub type SymbolSuggestionState = SuggestionState<SymbolSuggestionItem>;

/// A single file suggestion item for display.
#[derive(Debug, Clone)]
pub struct FileSuggestionItem {
    /// The file path (relative).
    pub path: String,
    /// Display text (may differ from path, e.g., with trailing / for dirs).
    pub display_text: String,
    /// Relevance score (higher = better match).
    pub score: u32,
    /// Character indices that matched the query (for highlighting).
    pub match_indices: Vec<i32>,
    /// Whether this is a directory.
    pub is_directory: bool,
}

/// A single skill suggestion item for display.
#[derive(Debug, Clone)]
pub struct SkillSuggestionItem {
    /// Skill name (e.g., "commit").
    pub name: String,
    /// Short description.
    pub description: String,
    /// Fuzzy match score (lower = better match).
    pub score: i32,
    /// Character indices that matched the query (for highlighting).
    pub match_indices: Vec<usize>,
}

/// A single agent suggestion item for display.
#[derive(Debug, Clone)]
pub struct AgentSuggestionItem {
    /// Agent type identifier (e.g., "explore").
    pub agent_type: String,
    /// Human-readable name (e.g., "Explore").
    pub name: String,
    /// Short description.
    pub description: String,
    /// Fuzzy match score (lower = better match).
    pub score: i32,
    /// Character indices that matched the query (for highlighting).
    pub match_indices: Vec<usize>,
}

/// A single symbol suggestion item for display.
#[derive(Debug, Clone)]
pub struct SymbolSuggestionItem {
    /// Symbol name (original case).
    pub name: String,
    /// Kind of symbol.
    pub kind: cocode_symbol_search::SymbolKind,
    /// File path relative to root.
    pub file_path: String,
    /// Line number (1-indexed).
    pub line: i32,
    /// Fuzzy match score (higher = better match).
    pub score: i32,
    /// Character indices that matched the query (for highlighting).
    pub match_indices: Vec<usize>,
}

/// Action resulting from the rewind selector overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewindAction {
    /// Rewind to a specific turn with a given mode.
    Rewind {
        /// The turn number to rewind to.
        turn_number: i32,
        /// The rewind mode.
        mode: cocode_protocol::RewindMode,
    },
    /// Summarize (partial compact) from a specific turn.
    Summarize {
        /// The turn number from which to summarize.
        turn_number: i32,
        /// Optional user-provided context to guide the summary focus.
        context: Option<String>,
    },
}

/// Phase of the rewind selector overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewindSelectorPhase {
    /// Selecting which checkpoint to rewind to.
    SelectCheckpoint,
    /// Selecting the rewind mode (code+conversation, code-only, conversation-only).
    SelectMode,
    /// Inputting optional context for the "Summarize from here" action.
    InputSummarizeContext,
}

/// Rewind selector overlay state.
#[derive(Debug, Clone)]
pub struct RewindSelectorOverlay {
    /// Available checkpoints (oldest first).
    pub checkpoints: Vec<cocode_protocol::RewindCheckpointItem>,
    /// Currently selected checkpoint index.
    pub selected: i32,
    /// Current phase.
    pub phase: RewindSelectorPhase,
    /// Selected mode index (in mode selection phase).
    pub mode_selected: i32,
    /// User-provided context for summarization.
    pub summarize_context: String,
    /// Turn number selected for summarization (saved when transitioning to context input).
    pub summarize_turn: Option<i32>,
    /// Whether a rewind/summarize operation is in progress.
    pub loading: bool,
    /// Description of the loading action (for display).
    pub loading_action: Option<String>,
    /// Whether the initially selected checkpoint needs diff stats fetched.
    /// Set to true when the overlay opens; consumed by the first navigation event.
    pub needs_initial_diff_stats: bool,
}

impl RewindSelectorOverlay {
    /// Create a new rewind selector. Items are displayed newest-first (reversed).
    pub fn new(checkpoints: Vec<cocode_protocol::RewindCheckpointItem>) -> Self {
        // Display order is newest-first, so index 0 = most recent checkpoint
        Self {
            checkpoints,
            selected: 0,
            phase: RewindSelectorPhase::SelectCheckpoint,
            mode_selected: 0,
            summarize_context: String::new(),
            summarize_turn: None,
            loading: false,
            loading_action: None,
            needs_initial_diff_stats: false,
        }
    }

    /// Get checkpoints in display order (newest first).
    pub fn display_items(&self) -> Vec<&cocode_protocol::RewindCheckpointItem> {
        self.checkpoints.iter().rev().collect()
    }

    /// Get the actual index into `checkpoints` for a display-order index.
    fn display_to_actual(&self, display_idx: i32) -> i32 {
        (self.checkpoints.len() as i32) - 1 - display_idx
    }

    /// Get the currently selected checkpoint.
    pub fn selected_checkpoint(&self) -> Option<&cocode_protocol::RewindCheckpointItem> {
        let actual = self.display_to_actual(self.selected);
        self.checkpoints.get(actual as usize)
    }

    /// Move selection up (toward newer checkpoints).
    pub fn move_up(&mut self) {
        match self.phase {
            RewindSelectorPhase::SelectCheckpoint => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            RewindSelectorPhase::SelectMode => {
                if self.mode_selected > 0 {
                    self.mode_selected -= 1;
                }
            }
            RewindSelectorPhase::InputSummarizeContext => {
                // No navigation in text input phase
            }
        }
    }

    /// Move selection down (toward older checkpoints).
    pub fn move_down(&mut self) {
        match self.phase {
            RewindSelectorPhase::SelectCheckpoint => {
                let max = (self.checkpoints.len() as i32).saturating_sub(1);
                if self.selected < max {
                    self.selected += 1;
                }
            }
            RewindSelectorPhase::SelectMode => {
                if self.mode_selected < 3 {
                    // 4 options: 0=CodeAndConversation, 1=ConversationOnly, 2=CodeOnly, 3=Summarize
                    self.mode_selected += 1;
                }
            }
            RewindSelectorPhase::InputSummarizeContext => {
                // No navigation in text input phase
            }
        }
    }

    /// Confirm selection and advance to next phase, or return the final choice.
    ///
    /// Returns `Some(RewindAction)` when ready to execute.
    pub fn confirm(&mut self) -> Option<RewindAction> {
        if self.loading {
            return None;
        }
        match self.phase {
            RewindSelectorPhase::SelectCheckpoint => {
                // Fast-path: if no file changes for this checkpoint, skip mode
                // selection and go directly to ConversationOnly rewind.
                let has_file_changes = self
                    .selected_checkpoint()
                    .map(|cp| cp.file_count > 0)
                    .unwrap_or(false);
                if !has_file_changes {
                    return self.selected_checkpoint().map(|cp| RewindAction::Rewind {
                        turn_number: cp.turn_number,
                        mode: cocode_protocol::RewindMode::ConversationOnly,
                    });
                }
                // Normal path: advance to mode selection
                self.phase = RewindSelectorPhase::SelectMode;
                self.mode_selected = 0;
                None
            }
            RewindSelectorPhase::SelectMode => {
                if self.mode_selected == 3 {
                    // Transition to context input phase for summarize
                    let turn = self.selected_checkpoint().map(|cp| cp.turn_number);
                    if let Some(t) = turn {
                        self.summarize_turn = Some(t);
                        self.summarize_context.clear();
                        self.phase = RewindSelectorPhase::InputSummarizeContext;
                    }
                    None
                } else {
                    let mode = match self.mode_selected {
                        0 => cocode_protocol::RewindMode::CodeAndConversation,
                        1 => cocode_protocol::RewindMode::ConversationOnly,
                        2 => cocode_protocol::RewindMode::CodeOnly,
                        _ => cocode_protocol::RewindMode::CodeAndConversation,
                    };
                    self.selected_checkpoint().map(|cp| RewindAction::Rewind {
                        turn_number: cp.turn_number,
                        mode,
                    })
                }
            }
            RewindSelectorPhase::InputSummarizeContext => {
                // Confirm summarize with the provided context
                self.summarize_turn.map(|turn_number| {
                    let ctx = self.summarize_context.trim().to_string();
                    RewindAction::Summarize {
                        turn_number,
                        context: if ctx.is_empty() { None } else { Some(ctx) },
                    }
                })
            }
        }
    }

    /// Go back to the previous phase.
    pub fn go_back(&mut self) -> bool {
        if self.loading {
            return false;
        }
        match self.phase {
            RewindSelectorPhase::SelectMode => {
                self.phase = RewindSelectorPhase::SelectCheckpoint;
                true
            }
            RewindSelectorPhase::InputSummarizeContext => {
                self.phase = RewindSelectorPhase::SelectMode;
                self.summarize_context.clear();
                true
            }
            RewindSelectorPhase::SelectCheckpoint => false,
        }
    }

    /// Insert a character into the summarize context input.
    pub fn insert_context_char(&mut self, c: char) {
        self.summarize_context.push(c);
    }

    /// Delete the last character from the summarize context input.
    pub fn delete_context_char(&mut self) {
        self.summarize_context.pop();
    }

    /// Set the loading state with an action description.
    pub fn set_loading(&mut self, action: String) {
        self.loading = true;
        self.loading_action = Some(action);
    }
}

#[cfg(test)]
#[path = "ui.test.rs"]
mod tests;
