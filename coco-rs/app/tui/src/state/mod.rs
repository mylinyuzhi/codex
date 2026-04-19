//! TUI application state — the Model in TEA.
//!
//! Split into two substates:
//! - [`SessionState`]: agent-synchronized data (model, messages, tools, subagents)
//! - [`UiState`]: local TUI state (input, scroll, overlay, streaming, theme)

pub mod overlay;
pub mod rewind;
pub mod session;
pub mod ui;

pub use overlay::AutoModeOptInOverlay;
pub use overlay::BridgeOverlay;
pub use overlay::BypassPermissionsOverlay;
pub use overlay::CommandOption;
pub use overlay::CommandPaletteOverlay;
pub use overlay::CostWarningOverlay;
pub use overlay::DiffViewOverlay;
pub use overlay::DoctorCheck;
pub use overlay::DoctorOverlay;
pub use overlay::ElicitationField;
pub use overlay::ElicitationOverlay;
pub use overlay::ExportFormat;
pub use overlay::ExportOverlay;
pub use overlay::FeedbackOverlay;
pub use overlay::GlobalSearchOverlay;
pub use overlay::IdleReturnOverlay;
pub use overlay::InvalidConfigOverlay;
pub use overlay::McpServerApprovalOverlay;
pub use overlay::McpServerOption;
pub use overlay::McpServerSelectOverlay;
pub use overlay::ModelOption;
pub use overlay::ModelPickerOverlay;
pub use overlay::Overlay;
pub use overlay::PermissionDetail;
pub use overlay::PermissionOverlay;
pub use overlay::PlanEntryOverlay;
pub use overlay::PlanExitOverlay;
pub use overlay::PlanExitTarget;
pub use overlay::QuestionOverlay;
pub use overlay::QuickOpenOverlay;
pub use overlay::RiskLevel;
pub use overlay::SandboxPermissionOverlay;
pub use overlay::SearchResult;
pub use overlay::SessionBrowserOverlay;
pub use overlay::SessionOption;
pub use overlay::TaskDetailOverlay;
pub use overlay::TrustOverlay;
pub use overlay::WorktreeExitOverlay;
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
pub use ui::ActiveSuggestions;
pub use ui::FocusTarget;
pub use ui::HistoryEntry;
pub use ui::InputState;
pub use ui::StreamMode;
pub use ui::StreamingState;
pub use ui::SuggestionKind;
pub use ui::Toast;
pub use ui::ToastSeverity;
pub use ui::UiState;

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

    /// Cycle permission mode (Shift+Tab).
    ///
    /// Delegates to [`PermissionMode::next_in_cycle`] so the TUI cycle
    /// stays aligned with `core/permissions::get_next_permission_mode`
    /// and the TS reference. Bypass/auto gate flags are forwarded from
    /// the session.
    pub fn cycle_permission_mode(&mut self) {
        self.session.permission_mode = self.session.permission_mode.next_in_cycle(
            self.session.bypass_permissions_available,
            self.session.auto_mode_available,
        );
    }

    /// Toggle plan mode on/off (Tab).
    ///
    /// Quick shortcut distinct from the full cycle: flips between
    /// `Plan` and `Default`, preserving nothing. Callers that need to
    /// return to an earlier elevated mode should use the full cycle.
    pub fn toggle_plan_mode(&mut self) {
        self.session.permission_mode = if self.session.permission_mode == PermissionMode::Plan {
            PermissionMode::Default
        } else {
            PermissionMode::Plan
        };
    }

    /// Whether the current session is in plan mode. Derived from
    /// [`SessionState::permission_mode`] — there is no separate bool.
    pub fn is_plan_mode(&self) -> bool {
        self.session.permission_mode == PermissionMode::Plan
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
