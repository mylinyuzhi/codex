//! TUI widgets.
//!
//! This module provides the main UI components:
//! - [`StatusBar`]: Displays model, thinking level, plan mode, and token usage
//! - [`ChatWidget`]: Displays the conversation history
//! - [`InputWidget`]: Multi-line input field
//! - [`ToolPanel`]: Shows tool execution progress
//! - [`FileSuggestionPopup`]: File autocomplete dropdown
//! - [`SkillSuggestionPopup`]: Skill autocomplete dropdown
//! - [`SubagentPanel`]: Subagent status display
//! - [`ToastWidget`]: Toast notification display

mod chat;
mod file_suggestion_popup;
mod input;
mod skill_suggestion_popup;
mod status_bar;
mod subagent_panel;
mod toast;
mod tool_panel;

pub use chat::ChatWidget;
pub use file_suggestion_popup::FileSuggestionPopup;
pub use input::InputWidget;
pub use skill_suggestion_popup::SkillSuggestionPopup;
pub use status_bar::StatusBar;
pub use subagent_panel::SubagentPanel;
pub use toast::Toast;
pub use toast::ToastSeverity;
pub use toast::ToastWidget;
pub use tool_panel::ToolPanel;
