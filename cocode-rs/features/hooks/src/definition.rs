//! Hook definition types.
//!
//! A `HookDefinition` describes a single hook: when it fires (event type),
//! what it matches against (optional matcher), and what it does (handler).

use serde::Deserialize;
use serde::Serialize;

use crate::event::HookEventType;
use crate::matcher::HookMatcher;
use crate::scope::HookSource;

/// Defines a single hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDefinition {
    /// The name of this hook (for logging and identification).
    pub name: String,

    /// The event type that triggers this hook.
    pub event_type: HookEventType,

    /// Optional matcher to filter which invocations trigger this hook.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<HookMatcher>,

    /// The handler to execute when this hook fires.
    pub handler: HookHandler,

    /// The source of this hook (determines scope/priority).
    #[serde(default)]
    pub source: HookSource,

    /// Whether this hook is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Timeout in seconds for hook execution.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: i32,

    /// If true, this hook is removed after a successful execution.
    ///
    /// One-shot hooks are useful for:
    /// - Running a lint check only once when skill starts
    /// - Initialization hooks that should not repeat
    /// - Hooks that should trigger exactly once per condition
    ///
    /// Note: The hook is only removed on successful execution (not on timeout or failure).
    #[serde(default)]
    pub once: bool,
}

fn default_enabled() -> bool {
    true
}

fn default_timeout_secs() -> i32 {
    30
}

/// The action performed by a hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookHandler {
    /// Run an external command.
    Command {
        /// The command to execute.
        command: String,
        /// Arguments for the command.
        #[serde(default)]
        args: Vec<String>,
    },

    /// Inject a prompt template.
    Prompt {
        /// Template string. `$ARGUMENTS` is replaced with the JSON context.
        template: String,
    },

    /// Delegate to a sub-agent.
    Agent {
        /// Maximum number of turns the agent can run.
        #[serde(default = "default_max_turns")]
        max_turns: i32,
    },

    /// Send an HTTP webhook.
    Webhook {
        /// The URL to call.
        url: String,
    },

    /// An inline function handler (not serializable).
    #[serde(skip)]
    Inline,
}

fn default_max_turns() -> i32 {
    5
}

#[cfg(test)]
#[path = "definition.test.rs"]
mod tests;
