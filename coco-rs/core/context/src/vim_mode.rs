//! Vim mode key handling.
//!
//! TS: vim/ (1.5K LOC) — vim emulation for text input.

/// Vim mode state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    Command,
}

impl Default for VimMode {
    fn default() -> Self {
        Self::Normal
    }
}

/// Vim key action result.
#[derive(Debug, Clone)]
pub enum VimAction {
    /// Insert character at cursor.
    InsertChar(char),
    /// Delete character at cursor.
    DeleteChar,
    /// Move cursor.
    MoveCursor(CursorMove),
    /// Switch mode.
    SwitchMode(VimMode),
    /// Execute ex command (from : prompt).
    ExCommand(String),
    /// No action (key consumed).
    None,
    /// Pass through to default handler.
    PassThrough,
}

/// Cursor movement direction.
#[derive(Debug, Clone, Copy)]
pub enum CursorMove {
    Left,
    Right,
    Up,
    Down,
    LineStart,
    LineEnd,
    WordForward,
    WordBackward,
    Top,
    Bottom,
}

/// Process a key press in vim mode.
pub fn process_vim_key(mode: VimMode, key: &str) -> (VimAction, VimMode) {
    match mode {
        VimMode::Normal => process_normal_key(key),
        VimMode::Insert => process_insert_key(key),
        VimMode::Visual => (VimAction::PassThrough, VimMode::Visual),
        VimMode::Command => (VimAction::PassThrough, VimMode::Command),
    }
}

fn process_normal_key(key: &str) -> (VimAction, VimMode) {
    match key {
        "i" => (VimAction::SwitchMode(VimMode::Insert), VimMode::Insert),
        "a" => (VimAction::MoveCursor(CursorMove::Right), VimMode::Insert),
        "I" => (
            VimAction::MoveCursor(CursorMove::LineStart),
            VimMode::Insert,
        ),
        "A" => (VimAction::MoveCursor(CursorMove::LineEnd), VimMode::Insert),
        "o" => (VimAction::InsertChar('\n'), VimMode::Insert),
        "h" => (VimAction::MoveCursor(CursorMove::Left), VimMode::Normal),
        "j" => (VimAction::MoveCursor(CursorMove::Down), VimMode::Normal),
        "k" => (VimAction::MoveCursor(CursorMove::Up), VimMode::Normal),
        "l" => (VimAction::MoveCursor(CursorMove::Right), VimMode::Normal),
        "w" => (
            VimAction::MoveCursor(CursorMove::WordForward),
            VimMode::Normal,
        ),
        "b" => (
            VimAction::MoveCursor(CursorMove::WordBackward),
            VimMode::Normal,
        ),
        "0" => (
            VimAction::MoveCursor(CursorMove::LineStart),
            VimMode::Normal,
        ),
        "$" => (VimAction::MoveCursor(CursorMove::LineEnd), VimMode::Normal),
        "x" => (VimAction::DeleteChar, VimMode::Normal),
        ":" => (VimAction::SwitchMode(VimMode::Command), VimMode::Command),
        "v" => (VimAction::SwitchMode(VimMode::Visual), VimMode::Visual),
        "escape" => (VimAction::None, VimMode::Normal),
        _ => (VimAction::None, VimMode::Normal),
    }
}

fn process_insert_key(key: &str) -> (VimAction, VimMode) {
    match key {
        "escape" => (VimAction::SwitchMode(VimMode::Normal), VimMode::Normal),
        "backspace" => (VimAction::DeleteChar, VimMode::Insert),
        k if k.len() == 1 => (
            VimAction::InsertChar(k.chars().next().unwrap()),
            VimMode::Insert,
        ),
        _ => (VimAction::PassThrough, VimMode::Insert),
    }
}

#[cfg(test)]
#[path = "vim_mode.test.rs"]
mod tests;
