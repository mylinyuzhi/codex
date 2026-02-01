//! TUI widgets.
//!
//! This module provides the main UI components:
//! - [`StatusBar`]: Displays model, thinking level, plan mode, and token usage
//! - [`ChatWidget`]: Displays the conversation history
//! - [`InputWidget`]: Multi-line input field
//! - [`ToolPanel`]: Shows tool execution progress
//! - [`FileSuggestionPopup`]: File autocomplete dropdown

mod chat;
mod file_suggestion_popup;
mod input;
mod status_bar;
mod tool_panel;

pub use chat::ChatWidget;
pub use file_suggestion_popup::FileSuggestionPopup;
pub use input::InputWidget;
pub use status_bar::StatusBar;
pub use tool_panel::ToolPanel;
