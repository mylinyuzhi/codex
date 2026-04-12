//! TUI widgets — ratatui components for each UI section.
//!
//! Each widget implements the `ratatui::widgets::Widget` trait
//! and is constructed via builder pattern from immutable state references.

mod chat;
mod context_viz;
mod coordinator_panel;
#[allow(dead_code)]
pub(crate) mod diff_display;
mod header_bar;
pub mod history_search;
pub mod ide_dialog;
mod input;
pub mod markdown;
pub mod plugin_manager;
mod progress_bar;
pub mod settings_panel;
mod status_bar;
mod subagent_panel;
pub mod suggestion_popup;
pub mod task_list;
mod team_status;
mod teammate_spinner;
mod teammate_view_header;
mod toast;
mod tool_panel;

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;

pub use chat::ChatWidget;
pub use context_viz::ContextVizWidget;
pub use coordinator_panel::CoordinatorPanel;
pub use coordinator_panel::CoordinatorTask;
pub use header_bar::HeaderBar;
pub use input::InputWidget;
pub use progress_bar::ProgressBarWidget;
pub use status_bar::StatusBar;
pub use subagent_panel::SubagentPanel;
pub use suggestion_popup::SuggestionPopup;
pub use team_status::TeamStatusWidget;
pub use teammate_spinner::TeammateSpinnerEntry;
pub use teammate_spinner::TeammateSpinnerTree;
pub use teammate_view_header::TeammateViewHeader;
pub use toast::ToastWidget;
pub use tool_panel::ToolPanel;
