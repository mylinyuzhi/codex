//! Keybinding configuration file types.
//!
//! Defines the JSON structure for `~/.cocode/keybindings.json`.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;

/// Root structure of `keybindings.json`.
///
/// ```json
/// {
///   "bindings": [
///     {
///       "context": "Chat",
///       "bindings": {
///         "ctrl+k ctrl+c": "ext:clearScreen",
///         "meta+p": "chat:modelPicker"
///       }
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeybindingsFile {
    /// Array of context-specific binding blocks.
    #[serde(default)]
    pub bindings: Vec<ContextBindings>,
}

/// A block of bindings for a single context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBindings {
    /// The context name (e.g., `"Chat"`, `"Global"`).
    pub context: String,
    /// Key-string -> action-string pairs.
    ///
    /// Values can be:
    /// - An action string like `"chat:submit"` or `"ext:togglePlanMode"`
    /// - A command string like `"command:doctor"` to bind to a slash command
    /// - `null` (in JSON) to explicitly unbind a key (represented as `None`)
    #[serde(default)]
    pub bindings: BTreeMap<String, Option<String>>,
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
