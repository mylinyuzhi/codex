//! Keybinding context — 18 user-rebindable + 2 internal contexts.
//!
//! TS sources:
//! - `keybindings/schema.ts:12-32` — the 18 publicly-validated contexts.
//! - `keybindings/defaultBindings.ts:196` (Scroll), `:271` (MessageActions)
//!   — internal-only contexts referenced by defaults but absent from the
//!   user-facing schema.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::str::FromStr;

/// A keybinding context — determines which bindings are active.
///
/// Wire format: PascalCase exactly as in TS (`Global`, `Chat`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeybindingContext {
    Global,
    Chat,
    Autocomplete,
    Confirmation,
    Help,
    Transcript,
    HistorySearch,
    Task,
    ThemePicker,
    Settings,
    Tabs,
    Attachments,
    Footer,
    MessageSelector,
    DiffDialog,
    ModelPicker,
    Select,
    Plugin,

    // Internal-only contexts — referenced by `defaultBindings.ts` but
    // not in the user-facing schema. The validator rejects user
    // bindings that target these.
    Scroll,
    MessageActions,
}

impl KeybindingContext {
    /// All 20 contexts (18 user + 2 internal). Iteration order matches
    /// TS `KEYBINDING_CONTEXTS`, with internal contexts appended.
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
        Self::Scroll,
        Self::MessageActions,
    ];

    /// The 18 contexts users may target in `keybindings.json`.
    /// Mirrors `KEYBINDING_CONTEXTS` from `keybindings/schema.ts:12-32`.
    pub const ALL_USER: &'static [Self] = &[
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

    /// Wire-format name (`"Global"`, `"Chat"`, …).
    pub fn as_str(self) -> &'static str {
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
            Self::Scroll => "Scroll",
            Self::MessageActions => "MessageActions",
        }
    }

    /// Human-readable description from `keybindings/schema.ts:37-58`.
    /// Internal-only contexts get a Rust-side description.
    pub fn description(self) -> &'static str {
        match self {
            Self::Global => "Active everywhere, regardless of focus",
            Self::Chat => "When the chat input is focused",
            Self::Autocomplete => "When autocomplete menu is visible",
            Self::Confirmation => "When a confirmation/permission dialog is shown",
            Self::Help => "When the help overlay is open",
            Self::Transcript => "When viewing the transcript",
            Self::HistorySearch => "When searching command history (ctrl+r)",
            Self::Task => "When a task/agent is running in the foreground",
            Self::ThemePicker => "When the theme picker is open",
            Self::Settings => "When the settings menu is open",
            Self::Tabs => "When tab navigation is active",
            Self::Attachments => "When navigating image attachments in a select dialog",
            Self::Footer => "When footer indicators are focused",
            Self::MessageSelector => "When the message selector (rewind) is open",
            Self::DiffDialog => "When the diff dialog is open",
            Self::ModelPicker => "When the model picker is open",
            Self::Select => "When a select/list component is focused",
            Self::Plugin => "When the plugin dialog is open",
            Self::Scroll => "Internal scroll-region bindings (not user-rebindable)",
            Self::MessageActions => "Internal message-actions menu bindings (not user-rebindable)",
        }
    }

    /// Whether users may target this context in `keybindings.json`.
    pub fn is_user_rebindable(self) -> bool {
        Self::ALL_USER.contains(&self)
    }
}

impl fmt::Display for KeybindingContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Returned when a string fails to parse as a [`KeybindingContext`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownContext {
    pub raw: String,
}

impl fmt::Display for UnknownContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown keybinding context `{}`", self.raw)
    }
}

impl std::error::Error for UnknownContext {}

impl FromStr for KeybindingContext {
    type Err = UnknownContext;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ctx = match s {
            "Global" => Self::Global,
            "Chat" => Self::Chat,
            "Autocomplete" => Self::Autocomplete,
            "Confirmation" => Self::Confirmation,
            "Help" => Self::Help,
            "Transcript" => Self::Transcript,
            "HistorySearch" => Self::HistorySearch,
            "Task" => Self::Task,
            "ThemePicker" => Self::ThemePicker,
            "Settings" => Self::Settings,
            "Tabs" => Self::Tabs,
            "Attachments" => Self::Attachments,
            "Footer" => Self::Footer,
            "MessageSelector" => Self::MessageSelector,
            "DiffDialog" => Self::DiffDialog,
            "ModelPicker" => Self::ModelPicker,
            "Select" => Self::Select,
            "Plugin" => Self::Plugin,
            "Scroll" => Self::Scroll,
            "MessageActions" => Self::MessageActions,
            other => {
                return Err(UnknownContext {
                    raw: other.to_string(),
                });
            }
        };
        Ok(ctx)
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
