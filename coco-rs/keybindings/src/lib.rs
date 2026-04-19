//! Keyboard shortcut management — 18 contexts, 50+ actions.
//!
//! TS: keybindings/ (context-based resolution, chord support, platform defaults)

pub mod parser;
pub mod resolver;
pub mod validator;

pub use parser::KeyChord;
pub use parser::KeyCombo;
pub use parser::parse_chord;
pub use parser::parse_combo;
pub use resolver::ChordResolver;
pub use resolver::ResolveOutcome;
pub use validator::ValidationIssue;
pub use validator::validate;

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;

/// A keybinding definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybinding {
    pub key: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
}

/// Load the default set of keybindings for common input/dialog/global contexts.
pub fn load_default_keybindings() -> Vec<Keybinding> {
    vec![
        Keybinding {
            key: "ctrl+c".into(),
            action: "interrupt".into(),
            context: Some("input".into()),
            when: None,
        },
        Keybinding {
            key: "ctrl+d".into(),
            action: "quit".into(),
            context: Some("input".into()),
            when: None,
        },
        Keybinding {
            key: "enter".into(),
            action: "submit".into(),
            context: Some("input".into()),
            when: None,
        },
        Keybinding {
            key: "escape".into(),
            action: "cancel".into(),
            context: Some("dialog".into()),
            when: None,
        },
        Keybinding {
            key: "tab".into(),
            action: "autocomplete".into(),
            context: Some("input".into()),
            when: None,
        },
        Keybinding {
            key: "ctrl+l".into(),
            action: "clear".into(),
            context: Some("global".into()),
            when: None,
        },
        Keybinding {
            key: "ctrl+o".into(),
            action: "compact".into(),
            context: Some("global".into()),
            when: None,
        },
    ]
}

/// Keybinding registry with context-based resolution.
#[derive(Default)]
pub struct KeybindingRegistry {
    bindings: Vec<Keybinding>,
    context_map: HashMap<String, Vec<usize>>,
}

impl KeybindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry pre-loaded with the default keybindings.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        for binding in load_default_keybindings() {
            registry.register(binding);
        }
        registry
    }

    pub fn register(&mut self, binding: Keybinding) {
        let idx = self.bindings.len();
        if let Some(ref ctx) = binding.context {
            self.context_map.entry(ctx.clone()).or_default().push(idx);
        }
        self.bindings.push(binding);
    }

    /// Resolve a key press in a given context.
    pub fn resolve(&self, key: &str, context: &str) -> Option<&str> {
        // Check context-specific bindings first
        if let Some(indices) = self.context_map.get(context) {
            for &idx in indices.iter().rev() {
                if self.bindings[idx].key == key {
                    return Some(&self.bindings[idx].action);
                }
            }
        }
        // Fall back to global bindings
        self.bindings
            .iter()
            .rev()
            .find(|b| b.key == key && b.context.is_none())
            .map(|b| b.action.as_str())
    }

    /// Return all keybindings matching a specific context.
    pub fn all_for_context(&self, context: &str) -> Vec<&Keybinding> {
        self.context_map
            .get(context)
            .map(|indices| indices.iter().map(|&idx| &self.bindings[idx]).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;

/// All known keybinding contexts.
///
/// TS: keybindings types (3.2K LOC)
pub const ALL_CONTEXTS: &[&str] = &[
    "global",
    "input",
    "conversation",
    "permission",
    "search",
    "plan",
    "diff",
    "agent",
    "worktree",
];

/// Default keybinding map — context → (key → action).
///
/// TS: getDefaultKeybindings()
pub fn get_all_defaults() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        // Global
        ("global", "ctrl+c", "interrupt"),
        ("global", "ctrl+d", "exit"),
        ("global", "ctrl+l", "clear_screen"),
        ("global", "ctrl+\\", "force_quit"),
        ("global", "escape", "cancel"),
        // Input
        ("input", "enter", "submit"),
        ("input", "shift+enter", "newline"),
        ("input", "up", "history_prev"),
        ("input", "down", "history_next"),
        ("input", "ctrl+r", "history_search"),
        ("input", "tab", "autocomplete"),
        ("input", "ctrl+a", "move_start"),
        ("input", "ctrl+e", "move_end"),
        ("input", "ctrl+u", "clear_line"),
        ("input", "ctrl+w", "delete_word"),
        ("input", "ctrl+k", "kill_to_end"),
        // Conversation
        ("conversation", "ctrl+c", "interrupt_generation"),
        ("conversation", "escape", "cancel_tool"),
        // Permission
        ("permission", "y", "approve"),
        ("permission", "n", "deny"),
        ("permission", "a", "approve_always"),
        ("permission", "escape", "deny"),
        // Search
        ("search", "enter", "select"),
        ("search", "escape", "close"),
        ("search", "up", "prev_result"),
        ("search", "down", "next_result"),
    ]
}

pub mod context;
pub use context::KeyContext;
pub use context::KeybindingResolver;
