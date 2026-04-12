//! Vim mode state machine.
//!
//! Complete vi input handling: normal/insert modes, motions, operators,
//! text objects, counts, find, and dot-repeat.
//!
//! TS: src/vim/ (5 files, 1513 LOC)

pub mod motions;
pub mod operators;
pub mod text_objects;
pub mod transitions;

/// Vim operator type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
}

/// Find motion type (f/F/t/T).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindType {
    /// Find forward to char (inclusive).
    F,
    /// Find backward to char (inclusive).
    BigF,
    /// Find forward till char (exclusive).
    T,
    /// Find backward till char (exclusive).
    BigT,
}

/// Text object scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObjScope {
    Inner,
    Around,
}

/// Complete vim state.
#[derive(Debug, Clone)]
pub enum VimState {
    /// Insert mode — keys go directly to input.
    Insert { inserted_text: String },
    /// Normal mode — keys are parsed as commands.
    Normal { command: CommandState },
}

/// Command state machine for normal mode.
#[derive(Debug, Clone)]
pub enum CommandState {
    /// Waiting for a command key.
    Idle,
    /// Accumulating count digits (e.g., "3" before a motion).
    Count { digits: String },
    /// Operator entered (d/c/y), waiting for motion or text object.
    OperatorPending { op: Operator, count: i32 },
    /// Operator + count digits (e.g., "d3" waiting for motion).
    OperatorCount {
        op: Operator,
        count: i32,
        digits: String,
    },
    /// Operator + find type, waiting for character.
    OperatorFind {
        op: Operator,
        count: i32,
        find: FindType,
    },
    /// Operator + text object scope (i/a), waiting for object key.
    OperatorTextObj {
        op: Operator,
        count: i32,
        scope: TextObjScope,
    },
    /// Find motion (f/F/t/T), waiting for character.
    Find { find: FindType, count: i32 },
    /// Waiting for second key after 'g'.
    G { count: i32 },
    /// Operator + g, waiting for second key.
    OperatorG { op: Operator, count: i32 },
    /// Replace mode (r), waiting for replacement character.
    Replace { count: i32 },
    /// Indent (> or <), waiting for motion.
    Indent { dir: IndentDir, count: i32 },
}

/// Indent direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentDir {
    Right,
    Left,
}

/// Persistent state that survives across commands (memory for repeats/pastes).
#[derive(Debug, Clone, Default)]
pub struct PersistentState {
    /// Last change for dot-repeat.
    pub last_change: Option<RecordedChange>,
    /// Last find motion for ; and , repeat.
    pub last_find: Option<(FindType, char)>,
    /// Yank register content.
    pub register: String,
    /// Whether register content is linewise.
    pub register_is_linewise: bool,
}

/// Recorded change for dot-repeat.
#[derive(Debug, Clone)]
pub enum RecordedChange {
    /// Text inserted in insert mode.
    Insert { text: String },
    /// Operator + motion executed.
    OperatorMotion {
        op: Operator,
        motion: String,
        count: i32,
    },
    /// Replace character.
    ReplaceChar { ch: char, count: i32 },
}

impl VimState {
    /// Create initial state (normal mode, idle).
    pub fn new() -> Self {
        VimState::Normal {
            command: CommandState::Idle,
        }
    }

    /// Whether currently in insert mode.
    pub fn is_insert(&self) -> bool {
        matches!(self, VimState::Insert { .. })
    }

    /// Whether currently in normal mode.
    pub fn is_normal(&self) -> bool {
        matches!(self, VimState::Normal { .. })
    }

    /// Enter insert mode.
    pub fn enter_insert(&mut self) {
        *self = VimState::Insert {
            inserted_text: String::new(),
        };
    }

    /// Enter normal mode, resetting command state.
    pub fn enter_normal(&mut self) {
        *self = VimState::Normal {
            command: CommandState::Idle,
        };
    }

    /// Get mode label for status bar.
    pub fn mode_label(&self) -> &str {
        match self {
            VimState::Insert { .. } => "INSERT",
            VimState::Normal {
                command: CommandState::Idle,
            } => "NORMAL",
            VimState::Normal { .. } => "NORMAL (pending)",
        }
    }
}

impl Default for VimState {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandState {
    /// Reset to idle.
    pub fn reset(&mut self) {
        *self = CommandState::Idle;
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
