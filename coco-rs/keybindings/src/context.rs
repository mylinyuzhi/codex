//! Context-aware keybinding resolution.
//!
//! TS: keybindings/ (3.2K LOC) — context-based keybinding lookup.

use std::collections::HashMap;

/// A keybinding context (determines which bindings are active).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyContext {
    Global,
    Input,
    Conversation,
    Permission,
    Search,
    Plan,
    Diff,
    Agent,
    Worktree,
}

impl KeyContext {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Input => "input",
            Self::Conversation => "conversation",
            Self::Permission => "permission",
            Self::Search => "search",
            Self::Plan => "plan",
            Self::Diff => "diff",
            Self::Agent => "agent",
            Self::Worktree => "worktree",
        }
    }
}

/// A resolved keybinding.
#[derive(Debug, Clone)]
pub struct Keybinding {
    pub key: String,
    pub action: String,
    pub context: KeyContext,
    pub description: Option<String>,
}

/// Keybinding resolver — looks up bindings by context + key.
#[derive(Default)]
pub struct KeybindingResolver {
    bindings: HashMap<(KeyContext, String), Keybinding>,
}

impl KeybindingResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a keybinding.
    pub fn register(&mut self, binding: Keybinding) {
        self.bindings
            .insert((binding.context, binding.key.clone()), binding);
    }

    /// Look up a keybinding by context and key.
    /// Falls back to Global context if not found in specific context.
    pub fn resolve(&self, context: KeyContext, key: &str) -> Option<&Keybinding> {
        self.bindings
            .get(&(context, key.to_string()))
            .or_else(|| self.bindings.get(&(KeyContext::Global, key.to_string())))
    }

    /// Get all bindings for a context (including global fallbacks).
    pub fn bindings_for_context(&self, context: KeyContext) -> Vec<&Keybinding> {
        let mut result: Vec<&Keybinding> = self
            .bindings
            .values()
            .filter(|b| b.context == context || b.context == KeyContext::Global)
            .collect();
        result.sort_by(|a, b| a.key.cmp(&b.key));
        result
    }

    /// Load default keybindings.
    pub fn load_defaults(&mut self) {
        for (ctx_str, key, action) in crate::get_all_defaults() {
            let context = match ctx_str {
                "global" => KeyContext::Global,
                "input" => KeyContext::Input,
                "conversation" => KeyContext::Conversation,
                "permission" => KeyContext::Permission,
                "search" => KeyContext::Search,
                _ => KeyContext::Global,
            };
            self.register(Keybinding {
                key: key.to_string(),
                action: action.to_string(),
                context,
                description: None,
            });
        }
    }
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
