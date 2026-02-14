//! Hook definition types.
//!
//! A `HookDefinition` describes a single hook: when it fires (event type),
//! what it matches against (optional matcher), and what it does (handler).

use serde::Deserialize;
use serde::Serialize;

use crate::event::HookEventType;
use crate::matcher::HookMatcher;
use crate::scope::HookSource;

/// Maximum allowed timeout in seconds (10 minutes).
pub const MAX_TIMEOUT_SECS: i32 = 600;

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

impl HookDefinition {
    /// Returns the effective timeout in seconds, clamped to [`MAX_TIMEOUT_SECS`].
    pub fn effective_timeout_secs(&self) -> i32 {
        self.timeout_secs.min(MAX_TIMEOUT_SECS)
    }
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

    /// Inject a prompt template or perform LLM verification.
    Prompt {
        /// Template string. `$ARGUMENTS` is replaced with the JSON context.
        template: String,
        /// Model to use for LLM verification mode.
        /// When set, the template is sent to this model for verification
        /// instead of simple template expansion.
        ///
        /// **Not yet effective** — currently ignored by the handler.
        /// Requires LLM callback injection into `HookRegistry`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
    },

    /// Delegate to a sub-agent for verification.
    ///
    /// **Not yet functional** — the handler is a stub that returns `Continue`.
    /// Requires a `SpawnAgentFn` callback to be injected into `HookRegistry`.
    Agent {
        /// Maximum number of turns the agent can run (capped at 50).
        #[serde(default = "default_max_turns")]
        max_turns: i32,
        /// Prompt template for the agent. `$ARGUMENTS` is replaced with context JSON.
        ///
        /// **Not yet effective** — currently ignored by the handler stub.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        /// Timeout in seconds for the agent handler (default: 60s).
        ///
        /// **Not yet effective** — currently ignored by the handler stub.
        #[serde(default = "default_agent_timeout_secs")]
        timeout: i32,
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
    50
}

fn default_agent_timeout_secs() -> i32 {
    60
}

#[cfg(test)]
#[path = "definition.test.rs"]
mod tests;
