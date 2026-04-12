//! TUI model (TEA state).

/// Spinner display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpinnerMode {
    #[default]
    Default,
    Tool {
        tool_name_hash: u64,
    },
    Thinking,
}

/// Display entry — a rendered message for the UI.
#[derive(Debug, Clone)]
pub struct DisplayEntry {
    pub role: DisplayRole,
    pub content: String,
    /// Tool name if this is a tool call/result.
    pub tool_name: Option<String>,
}

/// Who authored a display entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRole {
    User,
    Assistant,
    System,
    Tool,
}

/// TUI application model (TEA state).
#[derive(Debug, Default)]
pub struct AppModel {
    /// Rendered conversation entries.
    pub entries: Vec<DisplayEntry>,
    /// Current user input buffer.
    pub input: String,
    /// Input cursor position.
    pub cursor: usize,
    /// Whether agent is busy.
    pub is_busy: bool,
    /// Spinner mode.
    pub spinner: SpinnerMode,
    /// Scroll offset from bottom.
    pub scroll_offset: i32,
    /// Terminal dimensions.
    pub width: u16,
    pub height: u16,
    /// Model name for status bar.
    pub model: String,
    /// Session ID.
    pub session_id: String,
    /// Whether permission dialog is showing.
    pub permission_pending: Option<PermissionPrompt>,
    /// Error to display.
    pub error_message: Option<String>,
}

/// Active permission prompt.
#[derive(Debug, Clone)]
pub struct PermissionPrompt {
    pub tool_use_id: String,
    pub tool_name: String,
    pub message: String,
}

impl AppModel {
    /// Add a display entry.
    pub fn push_entry(&mut self, entry: DisplayEntry) {
        self.entries.push(entry);
    }

    /// Add assistant text.
    pub fn push_assistant_text(&mut self, text: &str) {
        self.entries.push(DisplayEntry {
            role: DisplayRole::Assistant,
            content: text.to_string(),
            tool_name: None,
        });
    }

    /// Add user text.
    pub fn push_user_text(&mut self, text: &str) {
        self.entries.push(DisplayEntry {
            role: DisplayRole::User,
            content: text.to_string(),
            tool_name: None,
        });
    }

    /// Add system message.
    pub fn push_system(&mut self, text: &str) {
        self.entries.push(DisplayEntry {
            role: DisplayRole::System,
            content: text.to_string(),
            tool_name: None,
        });
    }

    /// Insert character at cursor.
    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete character before cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.input[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.replace_range(prev..self.cursor, "");
            self.cursor = prev;
        }
    }

    /// Take the current input, clearing the buffer.
    pub fn take_input(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.input)
    }
}

#[cfg(test)]
#[path = "model.test.rs"]
mod tests;
