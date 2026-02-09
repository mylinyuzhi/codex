//! Plan mode state management.
//!
//! Tracks the state of plan mode across a session, including the active
//! plan file path and mode transitions.

use std::path::Path;
use std::path::PathBuf;

/// Plan mode state for a session.
///
/// Tracks whether plan mode is active, the current plan file path,
/// and state transitions for re-entry detection.
#[derive(Debug, Clone, Default)]
pub struct PlanModeState {
    /// Whether plan mode is currently active.
    pub is_active: bool,
    /// Path to the current plan file.
    pub plan_file_path: Option<PathBuf>,
    /// The plan slug for this session.
    pub plan_slug: Option<String>,
    /// Whether the user has exited plan mode at least once.
    pub has_exited: bool,
    /// Whether the exit notification needs to be attached (one-time).
    pub needs_exit_attachment: bool,
    /// Turn number when plan mode was entered.
    pub entered_at_turn: Option<i32>,
    /// Turn number when plan mode was exited.
    pub exited_at_turn: Option<i32>,
}

impl PlanModeState {
    /// Create a new empty plan mode state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter plan mode with the given plan file path and slug.
    pub fn enter(&mut self, plan_file_path: PathBuf, slug: String, turn: i32) {
        self.is_active = true;
        self.plan_file_path = Some(plan_file_path);
        self.plan_slug = Some(slug);
        self.entered_at_turn = Some(turn);
        self.needs_exit_attachment = false;
    }

    /// Exit plan mode.
    pub fn exit(&mut self, turn: i32) {
        self.is_active = false;
        self.has_exited = true;
        self.exited_at_turn = Some(turn);
        self.needs_exit_attachment = true;
    }

    /// Clear the exit attachment flag after it has been sent.
    pub fn clear_exit_attachment(&mut self) {
        self.needs_exit_attachment = false;
    }

    /// Check if this is a re-entry into plan mode.
    pub fn is_reentry(&self) -> bool {
        self.has_exited && self.is_active
    }

    /// Get the plan file path if in plan mode.
    pub fn get_plan_file(&self) -> Option<&Path> {
        if self.is_active {
            self.plan_file_path.as_deref()
        } else {
            None
        }
    }
}

/// Check if a file path is the current plan file (safe for writing in plan mode).
///
/// This function is used by the permission system to allow Write/Edit tool
/// access to the plan file even when in plan mode.
///
/// # Arguments
///
/// * `path` - The file path to check
/// * `plan_file_path` - The current plan file path (if in plan mode)
///
/// # Returns
///
/// `true` if the path matches the plan file, allowing write access.
pub fn is_safe_file(path: &Path, plan_file_path: Option<&Path>) -> bool {
    plan_file_path.is_some_and(|plan_path| path == plan_path)
}

#[cfg(test)]
#[path = "state.test.rs"]
mod tests;
