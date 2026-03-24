//! Keybinding contexts.
//!
//! Each UI state maps to a named context that determines which bindings
//! are active. `Global` bindings are always active regardless of context.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

/// Named contexts for keybinding resolution.
///
/// When resolving a key event, the resolver checks bindings for the
/// active context first, then falls back to `Global`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum KeybindingContext {
    /// Always active — bindings here apply in every state.
    Global,
    /// Chat input is focused (default state).
    Chat,
    /// Autocomplete menu is visible (file, skill, agent, or symbol).
    Autocomplete,
    /// Confirmation/permission dialog is shown.
    Confirmation,
    /// Help overlay is open.
    Help,
    /// Transcript view is active.
    Transcript,
    /// History search overlay (Ctrl+R).
    HistorySearch,
    /// A foreground task/agent is running.
    Task,
    /// Theme picker is open.
    ThemePicker,
    /// Settings menu is open.
    Settings,
    /// Tab navigation is active.
    Tabs,
    /// Attachment bar is focused.
    Attachments,
    /// Footer indicators are focused.
    Footer,
    /// Message selector (rewind) is open.
    MessageSelector,
    /// Diff dialog is open.
    DiffDialog,
    /// Model picker is open.
    ModelPicker,
    /// Generic select/list component is focused.
    Select,
    /// Plugin dialog is open.
    Plugin,
}

impl KeybindingContext {
    /// Canonical string representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Chat => "Chat",
            Self::Autocomplete => "Autocomplete",
            Self::Confirmation => "Confirmation",
            Self::Help => "Help",
            Self::Transcript => "Transcript",
            Self::HistorySearch => "HistorySearch",
            Self::Task => "Task",
            Self::ThemePicker => "ThemePicker",
            Self::Settings => "Settings",
            Self::Tabs => "Tabs",
            Self::Attachments => "Attachments",
            Self::Footer => "Footer",
            Self::MessageSelector => "MessageSelector",
            Self::DiffDialog => "DiffDialog",
            Self::ModelPicker => "ModelPicker",
            Self::Select => "Select",
            Self::Plugin => "Plugin",
        }
    }

    /// All valid context values.
    pub const ALL: &'static [Self] = &[
        Self::Global,
        Self::Chat,
        Self::Autocomplete,
        Self::Confirmation,
        Self::Help,
        Self::Transcript,
        Self::HistorySearch,
        Self::Task,
        Self::ThemePicker,
        Self::Settings,
        Self::Tabs,
        Self::Attachments,
        Self::Footer,
        Self::MessageSelector,
        Self::DiffDialog,
        Self::ModelPicker,
        Self::Select,
        Self::Plugin,
    ];
}

impl fmt::Display for KeybindingContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for KeybindingContext {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        for ctx in Self::ALL {
            if ctx.as_str().eq_ignore_ascii_case(s) {
                return Ok(*ctx);
            }
        }
        Err(format!("unknown context: {s}"))
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
