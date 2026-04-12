//! TUI application state — the Model in TEA.
//!
//! Split into two substates:
//! - [`SessionState`]: agent-synchronized data (model, messages, tools, subagents)
//! - [`UiState`]: local TUI state (input, scroll, overlay, streaming, theme)

pub mod rewind;
pub mod session;
pub mod ui;

pub use rewind::DiffStatsPreview;
pub use rewind::RestoreType;
pub use rewind::RewindOverlay;
pub use rewind::RewindPhase;
pub use rewind::RewindableMessage;
pub use session::ChatMessage;
pub use session::ChatRole;
pub use session::McpServerStatus;
pub use session::MessageContent;
pub use session::PlanAction;
pub use session::SavedSession;
pub use session::SessionState;
pub use session::SubagentInstance;
pub use session::SubagentStatus;
pub use session::TokenUsage;
pub use session::ToolExecution;
pub use session::ToolStatus;
pub use session::ToolUseStatus;
pub use ui::AutoModeOptInOverlay;
pub use ui::BridgeOverlay;
pub use ui::BypassPermissionsOverlay;
pub use ui::CommandPaletteOverlay;
pub use ui::DiffViewOverlay;
pub use ui::DoctorOverlay;
pub use ui::ElicitationOverlay;
pub use ui::ExportOverlay;
pub use ui::FeedbackOverlay;
pub use ui::FocusTarget;
pub use ui::GlobalSearchOverlay;
pub use ui::IdleReturnOverlay;
pub use ui::InputState;
pub use ui::InvalidConfigOverlay;
pub use ui::McpServerApprovalOverlay;
pub use ui::McpServerSelectOverlay;
pub use ui::ModelPickerOverlay;
pub use ui::Overlay;
pub use ui::PermissionOverlay;
pub use ui::PlanEntryOverlay;
pub use ui::QuestionOverlay;
pub use ui::QuickOpenOverlay;
pub use ui::SandboxPermissionOverlay;
pub use ui::SessionBrowserOverlay;
pub use ui::StreamMode;
pub use ui::StreamingState;
pub use ui::TaskDetailOverlay;
pub use ui::Toast;
pub use ui::ToastSeverity;
pub use ui::TrustOverlay;
pub use ui::UiState;
pub use ui::WorktreeExitOverlay;

use coco_types::PermissionMode;

/// Complete TUI application state.
#[derive(Debug)]
pub struct AppState {
    /// Agent-synchronized state.
    pub session: SessionState,
    /// UI-only local state.
    pub ui: UiState,
    /// Application lifecycle.
    pub running: RunningState,
}

/// Application lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunningState {
    #[default]
    Running,
    Done,
}

impl AppState {
    /// Create a new default state.
    pub fn new() -> Self {
        Self {
            session: SessionState::default(),
            ui: UiState::new(),
            running: RunningState::Running,
        }
    }

    /// Whether the app should exit.
    pub fn should_exit(&self) -> bool {
        self.running == RunningState::Done
    }

    /// Signal the app to exit.
    pub fn quit(&mut self) {
        self.running = RunningState::Done;
    }

    /// Whether an overlay is currently active.
    pub fn has_overlay(&self) -> bool {
        self.ui.overlay.is_some()
    }

    /// Whether the agent is actively streaming.
    pub fn is_streaming(&self) -> bool {
        self.ui.streaming.is_some()
    }

    /// Whether a spinner should be shown.
    pub fn should_show_spinner(&self) -> bool {
        self.is_streaming() || self.session.is_busy()
    }

    /// Cycle permission mode: default → plan → acceptEdits → default.
    pub fn cycle_permission_mode(&mut self) {
        self.session.permission_mode = match self.session.permission_mode {
            PermissionMode::Default => PermissionMode::Plan,
            PermissionMode::Plan => PermissionMode::AcceptEdits,
            _ => PermissionMode::Default,
        };
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
