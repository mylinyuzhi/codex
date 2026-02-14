//! TUI widgets.
//!
//! This module provides the main UI components:
//! - [`StatusBar`]: Displays model, thinking level, plan mode, and token usage
//! - [`ChatWidget`]: Displays the conversation history
//! - [`InputWidget`]: Multi-line input field
//! - [`ToolPanel`]: Shows tool execution progress
//! - [`HeaderBar`]: Session context header bar
//! - [`FileSuggestionPopup`]: File autocomplete dropdown
//! - [`SkillSuggestionPopup`]: Skill autocomplete dropdown
//! - [`AgentSuggestionPopup`]: Agent autocomplete dropdown
//! - [`SubagentPanel`]: Subagent status display
//! - [`ToastWidget`]: Toast notification display
//! - [`QueuedListWidget`]: Displays queued commands waiting to be processed

mod agent_suggestion_popup;
mod chat;
mod file_suggestion_popup;
mod header_bar;
mod input;
pub mod markdown;
mod queued_list;
mod skill_suggestion_popup;
mod status_bar;
mod subagent_panel;
mod symbol_suggestion_popup;
mod toast;
mod tool_panel;

pub use agent_suggestion_popup::AgentSuggestionPopup;
pub use chat::ChatWidget;
pub use file_suggestion_popup::FileSuggestionPopup;
pub use header_bar::HeaderBar;
pub use input::InputWidget;
pub use queued_list::QueuedListWidget;
pub use skill_suggestion_popup::SkillSuggestionPopup;
pub use status_bar::StatusBar;
pub use subagent_panel::SubagentPanel;
pub use symbol_suggestion_popup::SymbolSuggestionPopup;
pub use toast::Toast;
pub use toast::ToastSeverity;
pub use toast::ToastWidget;
pub use tool_panel::ToolPanel;
