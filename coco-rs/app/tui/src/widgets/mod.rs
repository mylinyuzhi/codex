//! TUI widgets — ratatui components for each UI section.
//!
//! Each widget implements the `ratatui::widgets::Widget` trait
//! and is constructed via builder pattern from immutable state references.

mod chat;
mod context_viz;
mod context_warning_banner;
mod coordinator_panel;
#[allow(dead_code)]
pub(crate) mod diff_display;
pub mod error_dialog;
mod header_bar;
pub mod history_search;
mod hook_status_panel;
pub mod ide_dialog;
mod input;
mod interrupt_banner;
mod lifecycle_banner;
mod local_command_log;
pub mod markdown;
mod mcp_status_panel;
mod model_fallback_banner;
pub mod notification;
mod permission_mode_banner;
pub mod plugin_manager;
mod progress_bar;
mod queue_status_widget;
mod rate_limit_panel;
pub mod settings_panel;
mod status_bar;
mod stream_stall_indicator;
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
pub use context_warning_banner::ContextWarningBanner;
pub use coordinator_panel::CoordinatorPanel;
pub use coordinator_panel::CoordinatorTask;
pub use header_bar::HeaderBar;
pub use hook_status_panel::HookStatusPanel;
pub use input::InputWidget;
pub use interrupt_banner::InterruptBanner;
pub use local_command_log::LocalCommandLog;
pub use mcp_status_panel::McpStatusPanel;
pub use model_fallback_banner::ModelFallbackBanner;
pub use permission_mode_banner::PermissionModeBanner;
pub use progress_bar::ProgressBarWidget;
pub use queue_status_widget::QueueStatusWidget;
pub use rate_limit_panel::RateLimitPanel;
pub use status_bar::StatusBar;
pub use stream_stall_indicator::StreamStallIndicator;
pub use subagent_panel::SubagentPanel;
pub use suggestion_popup::SuggestionPopup;
pub use team_status::TeamStatusWidget;
pub use teammate_spinner::TeammateSpinnerEntry;
pub use teammate_spinner::TeammateSpinnerTree;
pub use teammate_view_header::TeammateViewHeader;
pub use toast::ToastWidget;
pub use tool_panel::ToolPanel;
