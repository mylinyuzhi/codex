//! Application state management for the TUI.
//!
//! This module provides:
//! - [`AppState`]: The complete application state
//! - [`SessionState`]: State from the agent session
//! - [`UiState`]: UI-specific state (input, scroll, overlays)

mod session;
mod ui;

pub use session::BackgroundTask;
pub use session::BackgroundTaskStatus;
pub use session::ChatMessage;
pub use session::InlineToolCall;
pub use session::McpServerStatus;
pub use session::McpToolCall;
pub use session::MessageRole;
pub use session::PlanPhase;
pub use session::SessionState;
pub use session::SubagentInstance;
pub use session::SubagentStatus;
pub use session::ToolExecution;
pub use session::ToolStatus;
pub use ui::AgentSuggestionItem;
pub use ui::AgentSuggestionState;
pub use ui::CommandAction;
pub use ui::CommandItem;
pub use ui::CommandPaletteOverlay;
pub use ui::ElicitationField;
pub use ui::ElicitationFieldType;
pub use ui::ElicitationMode;
pub use ui::ElicitationOverlay;
pub use ui::FileSuggestionItem;
pub use ui::FileSuggestionState;
pub use ui::FocusTarget;
pub use ui::HistoryEntry;
pub use ui::InputState;
pub use ui::MarketplaceSummary;
pub use ui::ModelPickerOverlay;
pub use ui::OutputStylePickerItem;
pub use ui::OutputStylePickerOverlay;
pub use ui::Overlay;
pub use ui::PermissionOverlay;
pub use ui::PlanExitOverlay;
pub use ui::PluginErrorEntry;
pub use ui::PluginManagerOverlay;
pub use ui::PluginManagerTab;
pub use ui::PluginSummary;
pub use ui::QueryTiming;
pub use ui::QuestionOverlay;
pub use ui::RewindAction;
pub use ui::RewindSelectorOverlay;
pub use ui::RewindSelectorPhase;
pub use ui::SessionBrowserOverlay;
pub use ui::SessionSummary;
pub use ui::SkillSuggestionItem;
pub use ui::SkillSuggestionState;
pub use ui::StreamMode;
pub use ui::StreamingState;
pub use ui::StreamingToolUse;
pub use ui::SuggestionState;
pub use ui::SymbolSuggestionItem;
pub use ui::SymbolSuggestionState;
pub use ui::UiState;

// Re-export theme types for convenience
pub use crate::theme::Theme;
pub use crate::theme::ThemeName;

use cocode_protocol::ReasoningEffort;
use cocode_protocol::RoleSelection;
use cocode_protocol::ThinkingLevel;

/// The complete application state.
///
/// This is the "Model" in the Elm Architecture pattern. All application
/// state is contained here and updated immutably in response to events.
#[derive(Debug, Clone)]
pub struct AppState {
    /// Session state (from the agent).
    pub session: SessionState,

    /// UI state (local to the TUI).
    pub ui: UiState,

    /// Current running state.
    pub running: RunningState,
}

impl AppState {
    /// Create a new application state with default values.
    pub fn new() -> Self {
        Self {
            session: SessionState::default(),
            ui: UiState::default(),
            running: RunningState::Running,
        }
    }

    /// Create a new application state with the specified selection.
    pub fn with_selection(selection: RoleSelection) -> Self {
        let mut state = Self::new();
        state.session.current_selection = Some(selection);
        state
    }

    /// Check if the application should exit.
    pub fn should_exit(&self) -> bool {
        matches!(self.running, RunningState::Done)
    }

    /// Cycle permission mode: Default → AcceptEdits → Plan → Default.
    ///
    /// Updates both the permission mode and the plan_mode flag (plan_mode is
    /// true iff permission_mode == Plan).
    pub fn cycle_permission_mode(&mut self) {
        let new_mode = self.session.permission_mode.next_cycle();
        self.session.permission_mode = new_mode;
        self.session.plan_mode = new_mode == cocode_protocol::PermissionMode::Plan;
        tracing::info!(?new_mode, "Permission mode cycled");
    }

    /// Cycle to the next thinking level (model-aware).
    pub fn cycle_thinking_level(&mut self) {
        let Some(ref mut selection) = self.session.current_selection else {
            return;
        };

        let current_effort = selection.effective_thinking_level().effort;

        // Supported efforts from model; if unspecified, use full set
        let supported: Vec<ReasoningEffort> = selection
            .supported_thinking_levels
            .as_ref()
            .map(|levels| levels.iter().map(|l| l.effort).collect())
            .unwrap_or_else(|| {
                vec![
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                    ReasoningEffort::XHigh,
                ]
            });
        if supported.is_empty() {
            return;
        }

        // Cycle: None → supported[0] → ... → supported[N-1] → None
        let next_effort = if current_effort == ReasoningEffort::None {
            Some(supported[0])
        } else {
            match supported.iter().position(|&e| e == current_effort) {
                Some(i) if i + 1 < supported.len() => Some(supported[i + 1]),
                _ => None,
            }
        };

        match next_effort {
            Some(effort) => {
                let level = selection
                    .supported_thinking_levels
                    .as_ref()
                    .and_then(|levels| levels.iter().find(|l| l.effort == effort).cloned())
                    .unwrap_or_else(|| ThinkingLevel::new(effort));
                selection.set_thinking_level(level);
            }
            None => selection.clear_thinking_level(),
        }
        tracing::info!(
            thinking_level = ?selection.effective_thinking_level().effort,
            "Thinking level cycled"
        );
    }

    /// Set the running state to done.
    pub fn quit(&mut self) {
        self.running = RunningState::Done;
    }

    /// Check if there's an active overlay.
    pub fn has_overlay(&self) -> bool {
        self.ui.overlay.is_some()
    }

    /// Check if the agent is currently streaming a response.
    pub fn is_streaming(&self) -> bool {
        self.ui.streaming.is_some()
    }

    /// Whether the spinner animation should run and trigger redraws.
    ///
    /// Suppresses spinner when a blocking overlay (Permission, Question,
    /// Elicitation) is active, matching Claude Code's `showSpinner` logic.
    pub fn should_show_spinner(&self) -> bool {
        if !self.is_streaming() {
            return false;
        }
        !matches!(
            self.ui.overlay,
            Some(
                Overlay::Permission(_)
                    | Overlay::PlanExitApproval(_)
                    | Overlay::Question(_)
                    | Overlay::Elicitation(_)
            )
        )
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// The running state of the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunningState {
    /// The application is running normally.
    #[default]
    Running,

    /// The application is done and should exit.
    Done,
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
